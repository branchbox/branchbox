//! Database Module
//!
//! Manages database isolation and setup for feature worktrees:
//! - Database volume isolation per worktree
//! - Database setup commands (create, migrate, seed)
//! - Database cleanup during teardown
//! - Multiple database engine support (PostgreSQL, MySQL, MongoDB, etc.)

use super::Module;
use crate::{Error, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Database engine type
#[derive(Debug, Clone, PartialEq)]
pub enum DatabaseEngine {
    Postgres,
    MySQL,
    MongoDB,
    Unknown,
}

impl std::fmt::Display for DatabaseEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseEngine::Postgres => write!(f, "postgres"),
            DatabaseEngine::MySQL => write!(f, "mysql"),
            DatabaseEngine::MongoDB => write!(f, "mongodb"),
            DatabaseEngine::Unknown => write!(f, "unknown"),
        }
    }
}

/// Database module
pub struct DatabaseModule {
    enabled: bool,
    database_name: String,
    database_engine: DatabaseEngine,
    database_volume_name: String,
    setup_command: String,
}

impl DatabaseModule {
    /// Create a new Database module
    pub fn new() -> Self {
        Self {
            enabled: false,
            database_name: String::new(),
            database_engine: DatabaseEngine::Unknown,
            database_volume_name: String::new(),
            setup_command: String::new(),
        }
    }

    /// Detect database engine from compose file or config files
    fn detect_engine(&self, project_dir: &Path) -> DatabaseEngine {
        // Check compose file for database services
        let compose_file = project_dir.join(".devcontainer/compose.yaml");
        if compose_file.exists() {
            if let Ok(content) = fs::read_to_string(&compose_file) {
                if content.contains("postgres") {
                    return DatabaseEngine::Postgres;
                } else if content.contains("mysql") {
                    return DatabaseEngine::MySQL;
                } else if content.contains("mongodb") || content.contains("mongo:") {
                    return DatabaseEngine::MongoDB;
                }
            }
        }

        DatabaseEngine::Unknown
    }

    /// Get default setup command for engine
    fn default_setup_command(&self) -> String {
        match self.database_engine {
            DatabaseEngine::Postgres | DatabaseEngine::MySQL => "bin/rails db:setup".to_string(),
            _ => "echo 'No default setup command'".to_string(),
        }
    }

    /// Cleanup PostgreSQL database
    fn cleanup_postgres(&self) -> Result<()> {
        // Check if psql is available
        if Command::new("which").arg("psql").output().is_err() {
            tracing::info!("psql not available, database will be cleaned up with volume");
            return Ok(());
        }

        // Check if database exists
        let output = Command::new("psql").args(["-lqt"]).output();

        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains(&self.database_name) {
                tracing::info!("Dropping PostgreSQL database: {}", self.database_name);

                let result = Command::new("dropdb").arg(&self.database_name).output();

                if result.is_ok() {
                    tracing::info!("Database dropped successfully");
                } else {
                    tracing::warn!("Could not drop database (may have active connections)");
                }
            } else {
                tracing::info!("Database not found (may already be deleted)");
            }
        }

        Ok(())
    }

    /// Cleanup MySQL database
    fn cleanup_mysql(&self) -> Result<()> {
        // Check if mysql is available
        if Command::new("which").arg("mysql").output().is_err() {
            tracing::info!("mysql not available, database will be cleaned up with volume");
            return Ok(());
        }

        tracing::info!("Dropping MySQL database: {}", self.database_name);

        let sql = format!("DROP DATABASE IF EXISTS `{}`;", self.database_name);
        let result = Command::new("mysql").args(["-e", &sql]).output();

        if result.is_ok() {
            tracing::info!("Database dropped successfully");
        } else {
            tracing::warn!("Could not drop database");
        }

        Ok(())
    }

    /// Cleanup MongoDB database
    fn cleanup_mongodb(&self) -> Result<()> {
        // Check for mongo or mongosh
        let mongo_cmd = if Command::new("which").arg("mongosh").output().is_ok() {
            "mongosh"
        } else if Command::new("which").arg("mongo").output().is_ok() {
            "mongo"
        } else {
            tracing::info!("mongo/mongosh not available, database will be cleaned up with volume");
            return Ok(());
        };

        tracing::info!("Dropping MongoDB database: {}", self.database_name);

        let eval = format!("db.getSiblingDB('{}').dropDatabase()", self.database_name);
        let result = Command::new(mongo_cmd).args(["--eval", &eval]).output();

        if result.is_ok() {
            tracing::info!("Database dropped successfully");
        } else {
            tracing::warn!("Could not drop database");
        }

        Ok(())
    }
}

impl Default for DatabaseModule {
    fn default() -> Self {
        Self::new()
    }
}

impl Module for DatabaseModule {
    fn name(&self) -> &str {
        "database"
    }

    fn detect(&self, project_dir: &Path) -> bool {
        // Check for common database config files
        let config_files = ["config/database.yml", "prisma/schema.prisma", "knexfile.js"];

        for file in &config_files {
            if project_dir.join(file).exists() {
                return true;
            }
        }

        // Check compose file for database services
        let compose_file = project_dir.join(".devcontainer/compose.yaml");
        if compose_file.exists() {
            if let Ok(content) = fs::read_to_string(&compose_file) {
                if content.contains("postgres")
                    || content.contains("mysql")
                    || content.contains("mongodb")
                {
                    return true;
                }
            }
        }

        false
    }

    fn init(&mut self, main_dir: &Path, feature_dir: &Path) -> Result<()> {
        let work_feature = feature_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::validation("Invalid feature directory name".to_string()))?;

        // Generate database name (sanitize for database naming)
        let project_name =
            std::env::var("COMPOSE_PROJECT_NAME").unwrap_or_else(|_| "app".to_string());
        self.database_name = format!("{}_{}", project_name, work_feature.replace('-', "_"));

        // Detect database engine
        self.database_engine = self.detect_engine(main_dir);

        // Set default setup command
        self.setup_command = self.default_setup_command();

        // Generate volume name
        self.database_volume_name = format!("{}-{}-db-data", project_name, work_feature);

        tracing::info!("Database: {}", self.database_name);
        tracing::info!("Engine: {}", self.database_engine);
        tracing::info!("Volume: {}", self.database_volume_name);

        self.enabled = true;
        Ok(())
    }

    fn setup(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Setting up database configuration...");

        // Update .env with database-specific variables
        let env_file = feature_dir.join(".env");
        if env_file.exists() {
            let content = fs::read_to_string(&env_file)?;
            if !content.contains("DATABASE_NAME=") {
                let mut file = fs::OpenOptions::new().append(true).open(&env_file)?;

                use std::io::Write;
                writeln!(file)?;
                writeln!(
                    file,
                    "# Database configuration (managed by database module)"
                )?;
                writeln!(file, "DATABASE_NAME={}", self.database_name)?;

                tracing::info!("Added DATABASE_NAME to .env");
            }
        }

        tracing::info!("Database configuration ready");
        tracing::info!("After opening devcontainer, run: {}", self.setup_command);

        Ok(())
    }

    fn teardown(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Cleaning up database...");

        // Try to read database name from .env
        let env_file = feature_dir.join(".env");
        let db_name = if env_file.exists() {
            if let Ok(content) = fs::read_to_string(&env_file) {
                content
                    .lines()
                    .find(|line| line.starts_with("DATABASE_NAME="))
                    .and_then(|line| line.split('=').nth(1))
                    .map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
                    .unwrap_or_else(|| self.database_name.clone())
            } else {
                self.database_name.clone()
            }
        } else {
            self.database_name.clone()
        };

        if db_name.is_empty() {
            tracing::info!("No database name found, skipping cleanup");
            return Ok(());
        }

        tracing::info!("Database name: {}", db_name);

        // Cleanup based on engine
        match self.database_engine {
            DatabaseEngine::Postgres => self.cleanup_postgres()?,
            DatabaseEngine::MySQL => self.cleanup_mysql()?,
            DatabaseEngine::MongoDB => self.cleanup_mongodb()?,
            DatabaseEngine::Unknown => {
                tracing::info!("Unknown database engine, skipping cleanup");
                tracing::info!("You may need to drop database '{}' manually", db_name);
            }
        }

        tracing::info!("Database volume cleanup handled by docker compose");

        Ok(())
    }

    fn validate(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        let env_file = feature_dir.join(".env");

        if !env_file.exists() {
            tracing::warn!(".env file not found");
            return Ok(());
        }

        let content = fs::read_to_string(&env_file)?;
        if !content.contains("DATABASE_URL") && !content.contains("DATABASE_NAME") {
            tracing::warn!("No database configuration found in .env");
            tracing::info!("This may be OK if database config is in other files");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect_database_yml() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("config")).unwrap();
        std::fs::write(
            temp_dir.path().join("config/database.yml"),
            "production: {}",
        )
        .unwrap();

        let module = DatabaseModule::new();
        assert!(module.detect(temp_dir.path()));
    }

    #[test]
    fn test_detect_postgres_in_compose() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            temp_dir.path().join(".devcontainer/compose.yaml"),
            "services:\n  postgres:\n    image: postgres:14",
        )
        .unwrap();

        let module = DatabaseModule::new();
        assert!(module.detect(temp_dir.path()));
    }

    #[test]
    fn test_detect_engine_postgres() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            temp_dir.path().join(".devcontainer/compose.yaml"),
            "services:\n  postgres:\n    image: postgres:14",
        )
        .unwrap();

        let module = DatabaseModule::new();
        let engine = module.detect_engine(temp_dir.path());
        assert_eq!(engine, DatabaseEngine::Postgres);
    }

    #[test]
    fn test_init() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir(&feature_dir).unwrap();

        let mut module = DatabaseModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        assert!(module.enabled);
        assert!(!module.database_name.is_empty());
        assert!(!module.database_volume_name.is_empty());
    }

    #[test]
    fn test_default_setup_command() {
        let mut module = DatabaseModule::new();
        module.database_engine = DatabaseEngine::Postgres;
        assert_eq!(module.default_setup_command(), "bin/rails db:setup");

        module.database_engine = DatabaseEngine::Unknown;
        assert_eq!(
            module.default_setup_command(),
            "echo 'No default setup command'"
        );
    }

    #[test]
    fn test_name() {
        let module = DatabaseModule::new();
        assert_eq!(module.name(), "database");
    }

    #[test]
    fn test_default() {
        let module = DatabaseModule::default();
        assert_eq!(module.name(), "database");
        assert!(!module.enabled);
    }

    #[test]
    fn test_detect_prisma() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("prisma")).unwrap();
        std::fs::write(
            temp_dir.path().join("prisma/schema.prisma"),
            "datasource db {}",
        )
        .unwrap();

        let module = DatabaseModule::new();
        assert!(module.detect(temp_dir.path()));
    }

    #[test]
    fn test_detect_knexfile() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("knexfile.js"), "module.exports = {}").unwrap();

        let module = DatabaseModule::new();
        assert!(module.detect(temp_dir.path()));
    }

    #[test]
    fn test_detect_no_database() {
        let temp_dir = TempDir::new().unwrap();

        let module = DatabaseModule::new();
        assert!(!module.detect(temp_dir.path()));
    }

    #[test]
    fn test_detect_engine_mysql() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            temp_dir.path().join(".devcontainer/compose.yaml"),
            "services:\n  mysql:\n    image: mysql:8",
        )
        .unwrap();

        let module = DatabaseModule::new();
        let engine = module.detect_engine(temp_dir.path());
        assert_eq!(engine, DatabaseEngine::MySQL);
    }

    #[test]
    fn test_detect_engine_mongodb() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            temp_dir.path().join(".devcontainer/compose.yaml"),
            "services:\n  mongodb:\n    image: mongo:5",
        )
        .unwrap();

        let module = DatabaseModule::new();
        let engine = module.detect_engine(temp_dir.path());
        assert_eq!(engine, DatabaseEngine::MongoDB);
    }

    #[test]
    fn test_detect_engine_unknown() {
        let temp_dir = TempDir::new().unwrap();

        let module = DatabaseModule::new();
        let engine = module.detect_engine(temp_dir.path());
        assert_eq!(engine, DatabaseEngine::Unknown);
    }

    #[test]
    fn test_database_engine_display() {
        assert_eq!(DatabaseEngine::Postgres.to_string(), "postgres");
        assert_eq!(DatabaseEngine::MySQL.to_string(), "mysql");
        assert_eq!(DatabaseEngine::MongoDB.to_string(), "mongodb");
        assert_eq!(DatabaseEngine::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_setup_creates_env() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir(&feature_dir).unwrap();
        std::fs::write(feature_dir.join(".env"), "EXISTING=value\n").unwrap();

        let mut module = DatabaseModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();
        module.setup(main_dir.path(), &feature_dir).unwrap();

        let env_content = std::fs::read_to_string(feature_dir.join(".env")).unwrap();
        assert!(env_content.contains("DATABASE_NAME="));
    }

    #[test]
    fn test_setup_no_env_file() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir(&feature_dir).unwrap();

        let mut module = DatabaseModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Should not error if .env doesn't exist
        module.setup(main_dir.path(), &feature_dir).unwrap();
    }

    #[test]
    fn test_validate_no_env() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir(&feature_dir).unwrap();

        let module = DatabaseModule::new();
        // Should not error when no .env file
        module.validate(main_dir.path(), &feature_dir).unwrap();
    }

    #[test]
    fn test_validate_with_database_url() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir(&feature_dir).unwrap();
        std::fs::write(
            feature_dir.join(".env"),
            "DATABASE_URL=postgres://localhost/test",
        )
        .unwrap();

        let module = DatabaseModule::new();
        module.validate(main_dir.path(), &feature_dir).unwrap();
    }

    // Integration tests requiring Docker
    // Run with: cargo test -- --ignored

    #[test]
    #[ignore]
    fn test_cleanup_postgres_no_cli() {
        let module = DatabaseModule {
            enabled: true,
            database_name: "test_db".to_string(),
            database_engine: DatabaseEngine::Postgres,
            database_volume_name: "test-volume".to_string(),
            setup_command: String::new(),
        };

        // Should succeed even if psql isn't available
        let result = module.cleanup_postgres();
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_cleanup_mysql_no_cli() {
        let module = DatabaseModule {
            enabled: true,
            database_name: "test_db".to_string(),
            database_engine: DatabaseEngine::MySQL,
            database_volume_name: "test-volume".to_string(),
            setup_command: String::new(),
        };

        // Should succeed even if mysql isn't available
        let result = module.cleanup_mysql();
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_cleanup_mongodb_no_cli() {
        let module = DatabaseModule {
            enabled: true,
            database_name: "test_db".to_string(),
            database_engine: DatabaseEngine::MongoDB,
            database_volume_name: "test-volume".to_string(),
            setup_command: String::new(),
        };

        // Should succeed even if mongo/mongosh isn't available
        let result = module.cleanup_mongodb();
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_teardown_postgres() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-teardown-pg");
        std::fs::create_dir(&feature_dir).unwrap();
        std::fs::write(
            feature_dir.join(".env"),
            "DATABASE_NAME=branchbox_test_pg\n",
        )
        .unwrap();

        let mut module = DatabaseModule::new();
        module.database_engine = DatabaseEngine::Postgres;
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Teardown should succeed
        let result = module.teardown(main_dir.path(), &feature_dir);
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_teardown_mysql() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-teardown-mysql");
        std::fs::create_dir(&feature_dir).unwrap();
        std::fs::write(
            feature_dir.join(".env"),
            "DATABASE_NAME=branchbox_test_mysql\n",
        )
        .unwrap();

        let mut module = DatabaseModule::new();
        module.database_engine = DatabaseEngine::MySQL;
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Teardown should succeed
        let result = module.teardown(main_dir.path(), &feature_dir);
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_teardown_mongodb() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-teardown-mongo");
        std::fs::create_dir(&feature_dir).unwrap();
        std::fs::write(
            feature_dir.join(".env"),
            "DATABASE_NAME=branchbox_test_mongo\n",
        )
        .unwrap();

        let mut module = DatabaseModule::new();
        module.database_engine = DatabaseEngine::MongoDB;
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Teardown should succeed
        let result = module.teardown(main_dir.path(), &feature_dir);
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_teardown_unknown_engine() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-teardown-unknown");
        std::fs::create_dir(&feature_dir).unwrap();
        std::fs::write(
            feature_dir.join(".env"),
            "DATABASE_NAME=branchbox_test_unknown\n",
        )
        .unwrap();

        let mut module = DatabaseModule::new();
        module.database_engine = DatabaseEngine::Unknown;
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Teardown should succeed for unknown engine
        let result = module.teardown(main_dir.path(), &feature_dir);
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_teardown_no_env_file() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-teardown-no-env");
        std::fs::create_dir(&feature_dir).unwrap();

        let mut module = DatabaseModule::new();
        module.database_engine = DatabaseEngine::Postgres;
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Teardown should succeed even without .env
        let result = module.teardown(main_dir.path(), &feature_dir);
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_full_lifecycle() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-db-lifecycle");
        std::fs::create_dir(&feature_dir).unwrap();
        std::fs::create_dir_all(main_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            main_dir.path().join(".devcontainer/compose.yaml"),
            "services:\n  postgres:\n    image: postgres:14\n",
        )
        .unwrap();
        std::fs::write(feature_dir.join(".env"), "EXISTING=value\n").unwrap();

        let mut module = DatabaseModule::new();

        // Init
        module.init(main_dir.path(), &feature_dir).unwrap();
        assert!(module.enabled);
        assert_eq!(module.database_engine, DatabaseEngine::Postgres);

        // Setup
        module.setup(main_dir.path(), &feature_dir).unwrap();
        let env_content = std::fs::read_to_string(feature_dir.join(".env")).unwrap();
        assert!(env_content.contains("DATABASE_NAME="));

        // Validate
        module.validate(main_dir.path(), &feature_dir).unwrap();

        // Teardown
        module.teardown(main_dir.path(), &feature_dir).unwrap();
    }
}
