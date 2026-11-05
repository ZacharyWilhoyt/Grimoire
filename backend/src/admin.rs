//! Local operational commands. Passwords are read from a hidden terminal prompt.
use crate::auth;
use serde::Deserialize;
use sqlx::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

fn password() -> Result<String, String> {
    let first = rpassword::prompt_password("Password: ")
        .map_err(|_| "Could not read password from terminal.")?;
    auth::validate_password(&first).map_err(|error| error.1)?;
    let second = rpassword::prompt_password("Confirm password: ")
        .map_err(|_| "Could not read password from terminal.")?;
    if first != second {
        return Err("Passwords do not match.".into());
    }
    Ok(first)
}

pub async fn command(db: &PgPool, args: &[String]) -> Result<(), String> {
    if args == ["reset-managed-tls"] || args == ["enable-automatic-tls"] {
        let mut tx = db.begin().await.map_err(|_| "Database unavailable.")?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(0x4752494d544c53_i64)
            .execute(&mut *tx)
            .await
            .map_err(|_| "TLS settings unavailable.")?;
        if args[0] == "reset-managed-tls" {
            sqlx::query("UPDATE instance_security SET tls_settings=NULL,tls_revision=tls_revision+1 WHERE singleton").execute(&mut *tx).await.map_err(|_|"TLS reset failed.")?;
        } else {
            sqlx::query("UPDATE instance_security SET tls_settings=jsonb_set(tls_settings,'{automatic}','true'),tls_revision=tls_revision+1 WHERE singleton AND tls_settings IS NOT NULL").execute(&mut *tx).await.map_err(|_|"TLS update failed.")?;
        }
        sqlx::query("INSERT INTO auth_events(event) VALUES('operator_tls_recovery')")
            .execute(&mut *tx)
            .await
            .map_err(|_| "Audit failed.")?;
        tx.commit().await.map_err(|_| "TLS recovery failed.")?;
        if args[0] == "reset-managed-tls" {
            if crate::security::tls::restore_local_proxy().await? {
                println!("TLS settings reset and localhost HTTPS configuration applied to Caddy. Certificate files were preserved. Open https://localhost.");
            } else {
                println!("TLS settings reset in the selected database only. Managed Caddy is not configured in this process; run recovery inside the deployed API container to reset its proxy.");
            }
            return Ok(());
        }
        println!("TLS settings updated. Certificate files were preserved. See deployment recovery instructions.");
        return Ok(());
    }
    if args == ["reset-ip-access"] {
        let mut tx = db.begin().await.map_err(|_| "Database unavailable.")?;
        sqlx::query("UPDATE instance_security SET ip_policy='{\"default_action\":\"allow\",\"allow\":[],\"deny\":[]}'::jsonb,ip_revision=ip_revision+1,updated_at=now() WHERE singleton").execute(&mut *tx).await.map_err(|_|"IP reset failed.")?;
        sqlx::query("INSERT INTO auth_events(event) VALUES('operator_ip_policy_reset')")
            .execute(&mut *tx)
            .await
            .map_err(|_| "Audit failed.")?;
        tx.commit().await.map_err(|_| "IP reset failed.")?;
        println!("IP policy reset to unrestricted access. Authentication is still required.");
        return Ok(());
    }
    if args.len() != 2 {
        return Err("Usage: cargo run -- {bootstrap-admin|import-legacy-sources|reset-password|disable-user|enable-user} USERNAME".into());
    }
    let username = auth::username(&args[1]).map_err(|error| error.1)?;
    match args[0].as_str() {
        "bootstrap-admin" => {
            let hash = auth::hash_password(&password()?).map_err(|_| "Password hashing failed.")?;
            let mut tx = db.begin().await.map_err(|_| "Database unavailable.")?;
            sqlx::query("SELECT pg_advisory_xact_lock(71309211)")
                .execute(&mut *tx)
                .await
                .map_err(|_| "Database unavailable.")?;
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE is_admin)")
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| "Database unavailable.")?;
            if exists {
                return Err("An administrator already exists. Bootstrap cannot run again.".into());
            }
            let id = Uuid::new_v4();
            // Never promote an existing publicly registered account.
            sqlx::query(
                "INSERT INTO users (id,username,password_hash,is_admin) VALUES ($1,$2,$3,true)",
            )
            .bind(id)
            .bind(username)
            .bind(hash)
            .execute(&mut *tx)
            .await
            .map_err(|_| "Could not create administrator; the username may already exist.")?;
            sqlx::query("INSERT INTO auth_events(user_id,event) VALUES ($1,'admin_bootstrapped')")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|_| "Audit write failed.")?;
            tx.commit()
                .await
                .map_err(|_| "Could not commit administrator.")?;
            println!("Administrator created. Public registrations will receive regular accounts.");
        }
        "import-legacy-sources" => {
            #[derive(Deserialize)]
            struct LegacySource {
                id: String,
                name: String,
                path: String,
            }
            let admin: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM users WHERE username=$1 AND is_admin AND NOT disabled",
            )
            .bind(username)
            .fetch_optional(db)
            .await
            .map_err(|_| "Database unavailable.")?;
            let admin =
                admin.ok_or("Select an active administrator as the legacy source owner.")?;
            let directory = std::env::var_os("GRIMOIRE_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("data"));
            let contents = tokio::fs::read(directory.join("sources.json"))
                .await
                .map_err(|_| {
                    "Could not read legacy data/sources.json. Set GRIMOIRE_DATA_DIR if needed."
                })?;
            let sources: Vec<LegacySource> = serde_json::from_slice(&contents)
                .map_err(|_| "Legacy source registry is invalid.")?;
            let digest = auth::digest(
                std::str::from_utf8(&contents).map_err(|_| "Invalid registry encoding.")?,
            );
            let mut tx = db.begin().await.map_err(|_| "Database unavailable.")?;
            sqlx::query("INSERT INTO legacy_imports(digest,owner_id) VALUES ($1,$2)")
                .bind(digest)
                .bind(admin)
                .execute(&mut *tx)
                .await
                .map_err(|_| {
                    "This registry was already imported, or the database is unavailable."
                })?;
            for source in &sources {
                if !PathBuf::from(&source.path).is_absolute() {
                    return Err("Legacy registry contains a relative path; import aborted.".into());
                }
                sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,$3,$4)")
                    .bind(&source.id)
                    .bind(admin)
                    .bind(&source.name)
                    .bind(&source.path)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| "Source import conflict; no sources were imported.")?;
            }
            sqlx::query(
                "INSERT INTO auth_events(user_id,event) VALUES ($1,'legacy_sources_imported')",
            )
            .bind(admin)
            .execute(&mut *tx)
            .await
            .map_err(|_| "Audit write failed.")?;
            tx.commit().await.map_err(|_| "Source import failed.")?;
            println!(
                "Imported {} private sources. The original JSON file was preserved.",
                sources.len()
            );
        }
        "reset-password" | "disable-user" | "enable-user" => {
            let hash = if args[0] == "reset-password" {
                Some(auth::hash_password(&password()?).map_err(|_| "Password hashing failed.")?)
            } else {
                None
            };
            let mut tx = db.begin().await.map_err(|_| "Database unavailable.")?;
            let id: Option<Uuid> = sqlx::query_scalar("UPDATE users SET password_hash=COALESCE($2,password_hash), disabled=CASE WHEN $3='disable-user' THEN true WHEN $3='enable-user' THEN false ELSE disabled END WHERE username=$1 RETURNING id")
                .bind(username).bind(hash).bind(&args[0]).fetch_optional(&mut *tx).await.map_err(|_| "Could not update user.")?;
            let id = id.ok_or("User not found.")?;
            sqlx::query("DELETE FROM sessions WHERE user_id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|_| "Session revocation failed.")?;
            sqlx::query("INSERT INTO auth_events(user_id,event) VALUES ($1,$2)")
                .bind(id)
                .bind(format!("operator_{}", args[0]))
                .execute(&mut *tx)
                .await
                .map_err(|_| "Audit write failed.")?;
            tx.commit().await.map_err(|_| "Account update failed.")?;
            println!("Account updated and all its sessions revoked.");
        }
        _ => return Err("Unknown command.".into()),
    }
    Ok(())
}
