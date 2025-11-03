#!/bin/bash
# Smart naming utilities for git-worktree workflow
# Generates clean, dasherized feature names from human-readable titles

# Generate WORK_FEATURE name from title
# Smart truncation: removes filler words, takes max 4 meaningful words
# Usage: work_feature=$(generate_work_feature "OAuth Integration Feature")
# Returns: oauth-integration (lowercase, dasherized, no filler words)
generate_work_feature() {
  local title="$1"

  # Convert to lowercase, remove special chars
  local cleaned=$(echo "$title" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9 -]//g')

  # Remove common filler words
  local fillers="the|a|an|and|or|for|with|using|feature|integration|implementation|support|system"
  cleaned=$(echo "$cleaned" | sed -E "s/\b($fillers)\b//g" | tr -s ' ' | sed 's/^ //;s/ $//')

  # Take first 4 words, convert spaces to hyphens (idiomatic bash)
  local words=($cleaned)
  local result
  result=$(IFS=-; echo "${words[*]:0:4}")

  echo "$result"
}

# Validate WORK_FEATURE name (DNS-safe)
# Usage: validate_work_feature "oauth-integration" && echo "valid"
# Returns: 0 if valid, 1 if invalid
validate_work_feature() {
  local work_feature="$1"

  if [[ "$work_feature" =~ ^[a-z0-9-]+$ ]]; then
    return 0
  else
    return 1
  fi
}
