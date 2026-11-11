use super::*;
use crate::auth_integration::{body, register_user, request, state};
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;

#[test]
fn keys_are_scoped_to_the_provider_entity_level() {
    let mut input = Verification {
        location_id: Uuid::nil(),
        representation_id: Uuid::nil(),
        provider: Provider::TmdbTv,
        id: "00012".into(),
        season: None,
        episode: None,
        edition: "standard".into(),
        reason: "fixture".into(),
    };
    assert!(identity_key(&input).is_err());
    input.season = Some(2);
    input.episode = Some(4);
    let (entity, key, _) = identity_key(&input).unwrap();
    assert_eq!((entity, key.as_str()), ("episode", "12:2:4"));
    input.provider = Provider::TmdbMovie;
    assert!(identity_key(&input).is_err());
    input.season = None;
    input.episode = None;
    let (entity, key, _) = identity_key(&input).unwrap();
    assert_eq!((entity, key.as_str()), ("movie", "12"));
    input.provider = Provider::Openlibrary;
    input.id = "OL12M".into();
    assert!(identity_key(&input).is_err());
    input.id = "OL12W".into();
    assert_eq!(identity_key(&input).unwrap().0, "work");
    input.provider = Provider::Musicbrainz;
    input.id = Uuid::new_v4().to_string();
    assert_eq!(identity_key(&input).unwrap().0, "recording");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore]
async fn matching_is_permission_scoped_versioned_and_reversible(pool: PgPool) {
    let state = state(pool.clone());
    let (cookie, csrf, user) = register_user(&state, "match_owner").await;
    let (_, _, other) = register_user(&state, "match_other").await;
    let owner: Uuid = serde_json::from_value(user["id"].clone()).unwrap();
    let other: Uuid = serde_json::from_value(other["id"].clone()).unwrap();
    let origin = Some("http://localhost:5173");
    for (source, user) in [("owner", owner), ("private", other)] {
        sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES($1,$2,$1,'/'||$1)")
            .bind(source)
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
    }
    let node = Uuid::new_v4();
    let remote = Uuid::new_v4();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO source_nodes(id,owner_id,last_seen) VALUES($1,$2,now())")
        .bind(node)
        .bind(other)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sources(id,owner_id,name,kind,node_id)
                 VALUES('shared',$1,'Shared copy','remote_agent',$2)",
    )
    .bind(other)
    .bind(node)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO remote_catalog_state(source_id,revision,digest,published)
                 VALUES('shared',1,'fixture',true)",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO remote_media_items(source_id,id,name) VALUES('shared',$1,'Film.mp4')")
        .bind(remote)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO remote_media_representations(source_id,item_id,id,extension,size_bytes)
                 VALUES('shared',$1,$1,'mp4',150)",
    )
    .bind(remote)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO remote_source_grants(source_id,user_id,can_discover,can_access_media)
                 VALUES('shared',$1,true,false)",
    )
    .bind(owner)
    .execute(&mut *tx)
    .await
    .unwrap();
    crate::catalog_inventory::project_remote(&mut tx, "shared", &[remote])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    sqlx::query(
        "INSERT INTO source_permissions(source_id,user_id,can_discover) VALUES('shared',$1,true)",
    )
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();
    let observations = [
        ("owner", "Film.mkv", "mkv", 100),
        ("owner", "extras/Film.mkv", "mkv", 110),
        ("private", "Film.webm", "webm", 200),
    ];
    let entries: Vec<_> = observations
        .iter()
        .enumerate()
        .map(|(i, (source, path, ext, size))| {
            json!({"file_key":format!("{i:064x}"),"source_id":source,"path":path,
               "kind":"video","extension":ext,"size":size})
        })
        .collect();
    crate::catalog_inventory::record(&pool, json!(entries))
        .await
        .unwrap();
    let locations: Vec<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        "SELECT l.id,r.id,r.item_id FROM catalog_locations l
         JOIN catalog_representations r ON r.id=l.representation_id ORDER BY l.local_file_key",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let (first, extra, shared, hidden) = (locations[0], locations[1], locations[3], locations[2]);
    let list = "/api/catalog/v1/items";
    let preview_url = "/api/catalog/v1/review/preview";
    let write = "/api/catalog/v1/review";
    let suggestions = format!("/api/catalog/v1/locations/{}/suggestions", first.0);
    let get = |url: &str| {
        let url = url.to_owned();
        let state = &state;
        let cookie = &cookie;
        async move { request(state, Method::GET, &url, Some(cookie), None, None, None).await }
    };
    let post = |url, payload| {
        request(
            &state,
            Method::POST,
            url,
            Some(&cookie),
            Some(&csrf),
            origin,
            Some(payload),
        )
    };
    let page = body(get(list).await).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 3);
    let limited = body(get("/api/catalog/v1/items?limit=1").await).await;
    let next = limited["next"].as_str().unwrap();
    assert_eq!(
        body(get(&format!("{list}?after={next}")).await).await["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        get(&format!("{list}/{}", hidden.2)).await.status(),
        StatusCode::NOT_FOUND
    );
    let suggestions_json = body(get(&suggestions).await).await;
    assert_eq!(suggestions_json["suggestions"].as_array().unwrap().len(), 2);
    assert!(!suggestions_json.to_string().contains(&hidden.0.to_string()));

    let directory = body(
        get("/api/catalog/v1/sources/owner/review-candidates?pattern=directory&value=extras").await,
    )
    .await;
    assert_eq!(directory["locations"].as_array().unwrap().len(), 1);
    assert_eq!(directory["locations"][0]["location_id"], json!(extra.0));
    let report_url = format!("/api/catalog/v1/locations/{}/report", first.0);
    assert_eq!(
        body(post(&report_url, json!({"reason":"Request curator help"})).await).await["queued"],
        true
    );
    assert_eq!(
        body(post(&report_url, json!({"reason":"Duplicate case"})).await).await["queued"],
        false
    );
    assert_eq!(
        get("/api/admin/catalog/v1/cases").await.status(),
        StatusCode::FORBIDDEN
    );
    for _ in 0..3 {
        assert_eq!(
            post(&report_url, json!({"reason":"Repeat"})).await.status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        post(&report_url, json!({"reason":"Burst"})).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let mut change = json!({"changes":[{"location_id":first.0,"action":"decline","target":shared.0}],
                           "reason":"Title alone is ambiguous"});
    let plan = body(post(preview_url, change.clone()).await).await;
    change["changes"][0]["expected"] = plan[0]["expected"].clone();
    assert_eq!(body(post(write, change.clone()).await).await["changed"], 1);
    assert_eq!(body(post(write, change).await).await["changed"], 0);
    assert_eq!(
        body(get(&suggestions).await).await["suggestions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for (own, target) in [(shared.0, first.0), (first.0, hidden.0)] {
        assert_eq!(
            post(
                preview_url,
                json!({"changes":[{"location_id":own,"action":"link","target":target}],
                                            "reason":"disallowed"})
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }

    // Preview a batch with two files, then omit one as an explicit exception.
    let mut batch = json!({"changes":[{"location_id":first.0,"action":"link","target":shared.0},
                                     {"location_id":extra.0,"action":"link","target":shared.0}],
                           "reason":"Checked the files"});
    let plan = body(post(preview_url, batch.clone()).await).await;
    assert_eq!(plan.as_array().unwrap().len(), 2);
    batch["changes"].as_array_mut().unwrap().pop();
    batch["changes"][0]["expected"] = plan[0]["expected"].clone();
    let result = body(post(write, batch.clone()).await).await;
    assert_eq!(result["changed"], 1);
    assert_eq!(post(write, batch).await.status(), StatusCode::CONFLICT);
    assert_eq!(
        body(get(&format!("{list}/{}", first.2)).await).await["id"],
        json!(shared.2)
    );
    assert_eq!(
        body(get(&format!("{list}/{}", extra.2)).await).await["id"],
        json!(extra.2)
    );
    let reversal = format!("{write}/{}/undo", result["batch"].as_str().unwrap());
    assert_eq!(
        post(&reversal, json!({"reason":"Revert review"}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        body(get(&format!("{list}/{}", first.2)).await).await["id"],
        json!(first.2)
    );

    // A provider claim cannot affect grouping. Only the path called after
    // curator and provider verification can mint an active identity.
    sqlx::query(
        "INSERT INTO catalog_identity_evidence(id,location_id,origin,namespace,entity_type,identifier,verification)
         VALUES(gen_random_uuid(),$1,'agent','tmdb_movie','movie','42','asserted')",
    ).bind(hidden.0).execute(&pool).await.unwrap();
    assert_eq!(
        post(
            "/api/admin/catalog/v1/verify",
            json!({"location_id":first.0,"representation_id":first.1,
        "provider":"tmdb_movie","id":"42","edition":"theatrical","reason":"unprivileged"})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let mut db = pool.begin().await.unwrap();
    let own = location(&mut db, owner, first.0, true).await.unwrap();
    let remote_copy = location(&mut db, owner, shared.0, false).await.unwrap();
    let same = attest(
        &mut db,
        &own,
        "tmdb_movie",
        "movie",
        "42",
        "theatrical",
        owner,
        "provider stub",
    )
    .await
    .unwrap();
    assert_eq!(
        attest(
            &mut db,
            &remote_copy,
            "tmdb_movie",
            "movie",
            "42",
            "theatrical",
            owner,
            "provider stub"
        )
        .await
        .unwrap(),
        same
    );
    let extra_copy = location(&mut db, owner, extra.0, true).await.unwrap();
    assert_ne!(
        attest(
            &mut db,
            &extra_copy,
            "tmdb_movie",
            "movie",
            "42",
            "extended",
            owner,
            "provider stub"
        )
        .await
        .unwrap(),
        same
    );
    let own = location(&mut db, owner, first.0, true).await.unwrap();
    assert!(attest(
        &mut db,
        &own,
        "tmdb_movie",
        "movie",
        "43",
        "theatrical",
        owner,
        "conflicting"
    )
    .await
    .is_err());
    let retained: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM catalog_review_events WHERE location_id=$1
            AND action='verification_conflict' AND evidence_snapshot->>'identifier'='43'",
    )
    .bind(first.0)
    .fetch_one(&mut *db)
    .await
    .unwrap();
    assert_eq!(retained, 1);
    db.commit().await.unwrap();
    assert_eq!(
        body(get(list).await).await["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    sqlx::query("DELETE FROM source_permissions WHERE source_id='shared'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        get(&format!("{list}/{}", shared.2)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        body(get(&format!("{list}/{}", same)).await).await["location_id"],
        json!(first.0)
    );

    // The split switches to the original ID. Undo restores the prior group
    // without touching source-specific progress identifiers.
    sqlx::query("UPDATE users SET is_admin=true WHERE id=$1")
        .bind(owner)
        .execute(&pool)
        .await
        .unwrap();
    let cases = body(get("/api/admin/catalog/v1/cases").await).await;
    assert_eq!(cases["cases"].as_array().unwrap().len(), 1);
    let case = cases["cases"][0]["id"].as_str().unwrap();
    let resolve = format!("/api/admin/catalog/v1/cases/{case}/resolve");
    assert_eq!(
        post(&resolve, json!({"reason":"Reviewed the case"}))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let suppression = "/api/admin/catalog/v1/sources/owner/suppression";
    assert_eq!(
        post(
            suppression,
            json!({"reason":"Pause noisy matching","suppressed":true})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        body(get(&suggestions).await).await["suggestions"],
        json!([])
    );
    let mut split = json!({"curator":true,"changes":[{"location_id":first.0,"action":"split"}],"reason":"Check a contested cut"});
    let plan = body(post(preview_url, split.clone()).await).await;
    split["changes"][0]["expected"] = plan[0]["expected"].clone();
    let result: Value = body(post(write, split).await).await;
    assert_eq!(result["changed"], 1);
    assert_eq!(
        body(get(&format!("{list}/{}", first.2)).await).await["id"],
        json!(first.2)
    );
    let reversal = format!("{write}/{}/undo", result["batch"].as_str().unwrap());
    assert_eq!(
        post(&reversal, json!({"reason":"Restore verified cut"}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        body(get(&format!("{list}/{}", first.2)).await).await["id"],
        json!(same)
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore]
async fn local_full_hash_is_lazy_private_and_invalidated_by_changed_observation(pool: PgPool) {
    let state = state(pool.clone());
    let (cookie, csrf, owner) = register_user(&state, "hash_owner").await;
    let (visitor, _, _) = register_user(&state, "hash_visitor").await;
    let owner: Uuid = serde_json::from_value(owner["id"].clone()).unwrap();
    let root = std::env::temp_dir().join(format!("grimoire-hash-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let path = root.join("fixture.mkv");
    std::fs::write(&path, b"abc").unwrap();
    let modified = crate::modified_epoch(&std::fs::metadata(&path).unwrap()).unwrap();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES('hash',$1,'Hash fixture',$2)")
        .bind(owner)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let observe = |mtime| {
        json!([{"file_key":format!("{:064x}",1),"source_id":"hash",
        "path":"fixture.mkv","kind":"video","extension":"mkv","size":3,"modified_at":mtime}])
    };
    crate::catalog_inventory::record(&pool, observe(modified))
        .await
        .unwrap();
    let location: Uuid =
        sqlx::query_scalar("SELECT id FROM catalog_locations WHERE source_id='hash'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let url = format!("/api/catalog/v1/locations/{location}/hash");
    let read = || request(&state, Method::GET, &url, Some(&cookie), None, None, None);
    assert_eq!(body(read().await).await["state"], "idle");
    assert_eq!(
        request(&state, Method::GET, &url, Some(&visitor), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let queued = request(
        &state,
        Method::POST,
        &url,
        Some(&cookie),
        Some(&csrf),
        Some("http://localhost:5173"),
        None,
    )
    .await;
    assert_eq!(body(queued).await["state"], "running");
    let hashed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let status = body(read().await).await;
            if status["state"] != "running" {
                break status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        hashed["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    crate::catalog_inventory::record(&pool, observe(modified + 1.0))
        .await
        .unwrap();
    assert_eq!(body(read().await).await["state"], "idle");
}
