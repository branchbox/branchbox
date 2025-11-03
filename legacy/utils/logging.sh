#!/bin/bash
# Logging utilities for git-worktree workflow
# Provides colored output and standardized logging functions

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# Logging functions
log_header() {
  echo -e "\n${BLUE}${BOLD}==================================================${NC}"
  echo -e "${BLUE}${BOLD}  $1${NC}"
  echo -e "${BLUE}${BOLD}==================================================${NC}\n"
}

log_step() {
  echo -e "${YELLOW}[$1/$2] $3${NC}"
}

log_success() {
  echo -e "${GREEN}✓ $1${NC}"
}

log_error() {
  echo -e "${RED}✗ Error: $1${NC}"
}

log_info() {
  echo -e "${CYAN}ℹ $1${NC}"
}

log_warning() {
  echo -e "${YELLOW}⚠ Warning: $1${NC}"
}
