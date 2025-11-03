#!/bin/bash
# Database Module
# Manages database isolation and setup for feature worktrees
#
# This module handles:
# - Database volume isolation per worktree
# - Database setup commands (create, migrate, seed)
# - Database cleanup during teardown
# - Multiple database engine support (PostgreSQL, MySQL, MongoDB, etc.)

# Module metadata
MODULE_NAME="database"
MODULE_DESCRIPTION="Database isolation and setup management"
MODULE_VERSION="1.0.0"
MODULE_ENABLED=false
MODULE_DEPENDENCIES=()

# Module configuration (set during module_init)
MODULE_DATABASE_NAME=""
MODULE_DATABASE_ENGINE=""  # postgres, mysql, mongodb, etc.
MODULE_DATABASE_SETUP_COMMAND=""
MODULE_DATABASE_VOLUME_NAME=""

# Implementation of module interface

module_detect() {
  # Database module is optional and enabled if database is detected
  # Let the adapter determine if database setup is needed
  if command -v adapter_compose_services &> /dev/null; then
    local services=$(adapter_compose_services)
    if echo "$services" | grep -qE "postgres|mysql|mongodb"; then
      MODULE_ENABLED=true
      MODULE_CONFIDENCE=95
      return 0
    fi
  fi

  # Check for common database files
  if [ -f "config/database.yml" ] || [ -f "prisma/schema.prisma" ] || [ -f "knexfile.js" ]; then
    MODULE_ENABLED=true
    MODULE_CONFIDENCE=80
    return 0
  fi

  MODULE_ENABLED=false
  return 1
}

module_init() {
  local main_dir="$1"
  local feature_dir="$2"
  local config="${3:-}"

  local work_feature=$(basename "$feature_dir")

  # Generate database name (sanitize for database naming)
  MODULE_DATABASE_NAME="${COMPOSE_PROJECT_NAME:-app}_${work_feature//-/_}"

  # Detect database engine from adapter or compose services
  if command -v adapter_compose_services &> /dev/null; then
    local services=$(adapter_compose_services)
    if echo "$services" | grep -q "postgres"; then
      MODULE_DATABASE_ENGINE="postgres"
    elif echo "$services" | grep -q "mysql"; then
      MODULE_DATABASE_ENGINE="mysql"
    elif echo "$services" | grep -q "mongodb"; then
      MODULE_DATABASE_ENGINE="mongodb"
    else
      MODULE_DATABASE_ENGINE="unknown"
    fi
  else
    MODULE_DATABASE_ENGINE="unknown"
  fi

  # Get database setup command from adapter
  if command -v adapter_setup_database &> /dev/null; then
    MODULE_DATABASE_SETUP_COMMAND="adapter_setup_database"
  else
    # Default setup commands per engine
    case "$MODULE_DATABASE_ENGINE" in
      postgres|mysql)
        MODULE_DATABASE_SETUP_COMMAND="bin/rails db:setup"
        ;;
      *)
        MODULE_DATABASE_SETUP_COMMAND="echo 'No default setup command'"
        ;;
    esac
  fi

  # Generate volume name for database isolation
  MODULE_DATABASE_VOLUME_NAME="${COMPOSE_PROJECT_NAME:-app}-${work_feature}-db-data"

  module_log_step "$MODULE_NAME" "Database: $MODULE_DATABASE_NAME"
  module_log_step "$MODULE_NAME" "Engine: $MODULE_DATABASE_ENGINE"
  module_log_step "$MODULE_NAME" "Volume: $MODULE_DATABASE_VOLUME_NAME"

  return 0
}

module_setup() {
  local main_dir="$1"
  local feature_dir="$2"

  module_log_step "$MODULE_NAME" "Setting up database..."

  # The database setup should be done in the devcontainer after it starts
  # This function mainly provides instructions and validates configuration

  # Update .env with database-specific variables
  if [ -f "$feature_dir/.env" ]; then
    # Check if DATABASE_NAME is already set
    if ! grep -q "^DATABASE_NAME=" "$feature_dir/.env" 2>/dev/null; then
      cat >> "$feature_dir/.env" <<EOF

# Database configuration (managed by database module)
DATABASE_NAME=$MODULE_DATABASE_NAME
EOF
      module_log_step "$MODULE_NAME" "Added DATABASE_NAME to .env"
    fi
  fi

  module_log_success "$MODULE_NAME" "Database configuration ready"

  # Provide setup instructions
  echo ""
  echo -e "${CYAN}${BOLD}Database Setup Instructions:${NC}"
  echo ""
  echo -e "After opening the devcontainer, run:"
  echo -e "  ${BLUE}$MODULE_DATABASE_SETUP_COMMAND${NC}"
  echo ""

  return 0
}

module_teardown() {
  local main_dir="$1"
  local feature_dir="$2"

  module_log_step "$MODULE_NAME" "Cleaning up database..."

  # Extract database name from feature directory's .env if available
  local db_name="$MODULE_DATABASE_NAME"

  if [ -d "$feature_dir" ] && [ -f "$feature_dir/.env" ]; then
    db_name=$(grep "^DATABASE_NAME=" "$feature_dir/.env" 2>/dev/null | cut -d'=' -f2 | tr -d '"' | tr -d "'")
  fi

  if [ -z "$db_name" ]; then
    module_log_step "$MODULE_NAME" "No database name found, skipping cleanup"
    return 0
  fi

  module_log_step "$MODULE_NAME" "Database name: $db_name"

  # Database cleanup depends on the engine
  case "$MODULE_DATABASE_ENGINE" in
    postgres)
      _cleanup_postgres_database "$db_name"
      ;;
    mysql)
      _cleanup_mysql_database "$db_name"
      ;;
    mongodb)
      _cleanup_mongodb_database "$db_name"
      ;;
    *)
      module_log_step "$MODULE_NAME" "Unknown database engine, skipping cleanup"
      module_log_step "$MODULE_NAME" "You may need to drop database '$db_name' manually"
      ;;
  esac

  # Docker volumes are automatically removed by docker compose down -v
  module_log_step "$MODULE_NAME" "Database volume cleanup handled by docker compose"

  return 0
}

module_validate() {
  local main_dir="$1"
  local feature_dir="$2"

  # Validate database configuration in .env
  if [ ! -f "$feature_dir/.env" ]; then
    module_log_warning "$MODULE_NAME" ".env file not found"
    return 1
  fi

  # Check for database URL or connection settings
  if ! grep -qE "DATABASE_URL|DATABASE_NAME" "$feature_dir/.env" 2>/dev/null; then
    module_log_warning "$MODULE_NAME" "No database configuration found in .env"
    module_log_step "$MODULE_NAME" "This may be OK if database config is in other files"
  fi

  return 0
}

module_get_name() {
  echo "$MODULE_NAME"
}

module_get_description() {
  echo "$MODULE_DESCRIPTION"
}

module_get_version() {
  echo "$MODULE_VERSION"
}

module_health_check() {
  local feature_dir="$1"

  # Check if database is reachable (would need to be in devcontainer)
  # For now, just check if database config exists
  if [ -f "$feature_dir/.env" ] && grep -q "DATABASE" "$feature_dir/.env" 2>/dev/null; then
    return 0
  fi

  return 1
}

# Module-specific helper functions

# _cleanup_postgres_database() - Drop PostgreSQL database
#
# Args:
#   $1 - Database name
_cleanup_postgres_database() {
  local db_name="$1"

  # Check if psql is available (need to be on host or in devcontainer)
  if ! command -v psql &> /dev/null; then
    module_log_step "$MODULE_NAME" "psql not available on host, skipping database drop"
    module_log_step "$MODULE_NAME" "Database '$db_name' will be cleaned up when volume is removed"
    return 0
  fi

  # Check if database exists
  if psql -lqt 2>/dev/null | cut -d \| -f 1 | grep -qw "$db_name"; then
    module_log_step "$MODULE_NAME" "Dropping PostgreSQL database: $db_name"

    # Try to drop the database
    if dropdb "$db_name" 2>/dev/null; then
      module_log_success "$MODULE_NAME" "Database dropped successfully"
    else
      module_log_warning "$MODULE_NAME" "Could not drop database (may have active connections)"
      module_log_step "$MODULE_NAME" "Database will be cleaned up when Docker volume is removed"
    fi
  else
    module_log_step "$MODULE_NAME" "Database not found (may already be deleted)"
  fi
}

# _cleanup_mysql_database() - Drop MySQL database
#
# Args:
#   $1 - Database name
_cleanup_mysql_database() {
  local db_name="$1"

  # Check if mysql is available
  if ! command -v mysql &> /dev/null; then
    module_log_step "$MODULE_NAME" "mysql not available on host, skipping database drop"
    module_log_step "$MODULE_NAME" "Database '$db_name' will be cleaned up when volume is removed"
    return 0
  fi

  # Try to drop the database
  module_log_step "$MODULE_NAME" "Dropping MySQL database: $db_name"

  if mysql -e "DROP DATABASE IF EXISTS \`$db_name\`;" 2>/dev/null; then
    module_log_success "$MODULE_NAME" "Database dropped successfully"
  else
    module_log_warning "$MODULE_NAME" "Could not drop database"
    module_log_step "$MODULE_NAME" "Database will be cleaned up when Docker volume is removed"
  fi
}

# _cleanup_mongodb_database() - Drop MongoDB database
#
# Args:
#   $1 - Database name
_cleanup_mongodb_database() {
  local db_name="$1"

  # Check if mongo is available
  if ! command -v mongo &> /dev/null && ! command -v mongosh &> /dev/null; then
    module_log_step "$MODULE_NAME" "mongo/mongosh not available on host, skipping database drop"
    module_log_step "$MODULE_NAME" "Database '$db_name' will be cleaned up when volume is removed"
    return 0
  fi

  # Try to drop the database
  module_log_step "$MODULE_NAME" "Dropping MongoDB database: $db_name"

  local mongo_cmd="mongo"
  if command -v mongosh &> /dev/null; then
    mongo_cmd="mongosh"
  fi

  if $mongo_cmd --eval "db.getSiblingDB('$db_name').dropDatabase()" 2>/dev/null; then
    module_log_success "$MODULE_NAME" "Database dropped successfully"
  else
    module_log_warning "$MODULE_NAME" "Could not drop database"
    module_log_step "$MODULE_NAME" "Database will be cleaned up when Docker volume is removed"
  fi
}
