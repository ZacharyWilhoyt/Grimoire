\getenv app_password GRIMOIRE_DB_PASSWORD
CREATE ROLE grimoire LOGIN PASSWORD :'app_password' NOSUPERUSER NOCREATEDB NOCREATEROLE;
ALTER DATABASE grimoire OWNER TO grimoire;
\connect grimoire
GRANT ALL ON SCHEMA public TO grimoire;
