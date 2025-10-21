# Communication Protocol Specification

## gRPC Service Definitions

### Agent Service

The agent exposes a gRPC service for both local and remote communication.

```protobuf
syntax = "proto3";

package worktree.agent.v1;

// Agent service handles worktree lifecycle operations
service WorktreeAgent {
  // Worktree lifecycle
  rpc StartFeature(StartFeatureRequest) returns (Feature);
  rpc TeardownFeature(TeardownFeatureRequest) returns (TeardownResponse);
  rpc ListWorktrees(ListWorktreesRequest) returns (ListWorkTreesResponse);
  rpc GetWorktree(GetWorktreeRequest) returns (Feature);

  // State synchronization
  rpc ReportState(StateReport) returns (StateAck);
  rpc StreamState(stream StateUpdate) returns (stream StateCommand);

  // Health and info
  rpc GetAgentInfo(GetAgentInfoRequest) returns (AgentInfo);
  rpc Ping(PingRequest) returns (PongResponse);
}

// Start a new feature worktree
message StartFeatureRequest {
  string name = 1;                    // Feature name or search term
  bool accept_defaults = 2;           // Skip interactive prompts
  optional string cloudflare_token = 3; // Cloudflare tunnel token
  optional string base_branch = 4;    // Branch to fork from (default: main)
  map<string, string> env_vars = 5;   // Additional environment variables
}

// Feature worktree representation
message Feature {
  string name = 1;                    // work_feature name
  string branch = 2;                  // Git branch name
  string url = 3;                     // Cloudflare tunnel URL
  string worktree_path = 4;           // Absolute path to worktree
  FeatureStatus status = 5;           // Current status
  int64 created_at = 6;               // Unix timestamp
  int64 updated_at = 7;               // Unix timestamp
  FeatureMetadata metadata = 8;       // Additional metadata
}

enum FeatureStatus {
  FEATURE_STATUS_UNSPECIFIED = 0;
  FEATURE_STATUS_STARTING = 1;
  FEATURE_STATUS_RUNNING = 2;
  FEATURE_STATUS_STOPPED = 3;
  FEATURE_STATUS_ERROR = 4;
  FEATURE_STATUS_TEARDOWN = 5;
}

message FeatureMetadata {
  string compose_project_name = 1;   // Docker Compose project name
  repeated string container_names = 2; // Container names
  repeated string volume_names = 3;   // Volume names
  string adapter = 4;                 // Stack adapter (rails, nodejs, generic)
  map<string, string> env_vars = 5;   // Environment variables
}

// Teardown a feature worktree
message TeardownFeatureRequest {
  string name = 1;                    // work_feature name
  bool force = 2;                     // Force teardown even if errors
  bool remove_branch = 3;             // Also delete git branch
  bool remove_tunnel = 4;             // Also delete Cloudflare tunnel
}

message TeardownResponse {
  bool success = 1;
  string message = 2;
  repeated string errors = 3;
}

// List all worktrees
message ListWorkTreesRequest {
  optional FeatureStatus status_filter = 1;
}

message ListWorkTreesResponse {
  repeated Feature features = 1;
}

// Get single worktree
message GetWorktreeRequest {
  string name = 1;
}

// State reporting
message StateReport {
  string device_id = 1;
  repeated Feature features = 2;
  SystemInfo system = 3;
  int64 timestamp = 4;
}

message StateAck {
  bool accepted = 1;
  string message = 2;
}

// Bi-directional streaming for real-time sync
message StateUpdate {
  oneof update {
    FeatureUpdate feature_update = 1;
    SystemUpdate system_update = 2;
    LogUpdate log_update = 3;
  }
}

message FeatureUpdate {
  Feature feature = 1;
  UpdateType type = 2;

  enum UpdateType {
    UPDATE_TYPE_UNSPECIFIED = 0;
    UPDATE_TYPE_CREATED = 1;
    UPDATE_TYPE_UPDATED = 2;
    UPDATE_TYPE_DELETED = 3;
  }
}

message SystemUpdate {
  SystemInfo system = 1;
}

message LogUpdate {
  string level = 1;
  string message = 2;
  string feature_name = 3;
  int64 timestamp = 4;
}

message StateCommand {
  oneof command {
    StartFeatureRequest start_feature = 1;
    TeardownFeatureRequest teardown_feature = 2;
    SyncRequest sync = 3;
  }
}

message SyncRequest {
  bool force = 1;
}

// System information
message SystemInfo {
  string hostname = 1;
  string os = 2;
  string architecture = 3;
  string agent_version = 4;
  int64 uptime_seconds = 5;
  ResourceUsage resources = 6;
}

message ResourceUsage {
  float cpu_percent = 1;
  uint64 memory_bytes = 2;
  uint64 disk_bytes = 3;
}

// Agent information
message GetAgentInfoRequest {}

message AgentInfo {
  string version = 1;
  string device_id = 2;
  string device_name = 3;
  bool control_plane_connected = 4;
  optional string control_plane_url = 5;
  optional string tailscale_ip = 6;
  SystemInfo system = 7;
}

// Health check
message PingRequest {}

message PongResponse {
  int64 timestamp = 1;
}
```

## REST API (Control Plane)

### Device Management

#### Register Device

```http
POST /api/v1/devices/register
Content-Type: application/json

{
  "registration_code": "ABC123",
  "device_info": {
    "hostname": "MacBook Pro",
    "os": "darwin",
    "architecture": "arm64",
    "tailscale_ip": "100.64.0.1"
  }
}

Response 201 Created:
{
  "device": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "name": "MacBook Pro",
    "token": "secret-device-token",
    "tailscale_ip": "100.64.0.1",
    "status": "online"
  }
}
```

#### List Devices

```http
GET /api/v1/devices
Authorization: Bearer <user-token>

Response 200 OK:
{
  "devices": [
    {
      "id": "550e8400-...",
      "name": "MacBook Pro",
      "status": "online",
      "last_seen_at": "2025-10-21T10:30:00Z",
      "agent_version": "1.0.0",
      "worktrees_count": 3
    }
  ]
}
```

#### Delete Device

```http
DELETE /api/v1/devices/:id
Authorization: Bearer <user-token>

Response 204 No Content
```

### Worktree Management

#### List Worktrees on Device

```http
GET /api/v1/devices/:device_id/worktrees
Authorization: Bearer <user-token>

Response 200 OK:
{
  "worktrees": [
    {
      "id": "...",
      "name": "oauth-integration",
      "branch": "feature/oauth-integration",
      "url": "https://example.com",
      "status": "running",
      "created_at": "2025-10-21T10:00:00Z"
    }
  ]
}
```

#### Start Worktree (Remote Command)

```http
POST /api/v1/devices/:device_id/worktrees/start
Authorization: Bearer <user-token>
Content-Type: application/json

{
  "name": "oauth integration",
  "accept_defaults": true
}

Response 202 Accepted:
{
  "command_id": "cmd-123",
  "status": "queued"
}
```

#### Teardown Worktree (Remote Command)

```http
DELETE /api/v1/devices/:device_id/worktrees/:name
Authorization: Bearer <user-token>

Response 202 Accepted:
{
  "command_id": "cmd-124",
  "status": "queued"
}
```

### Agent Endpoints

#### Heartbeat

```http
POST /agent/heartbeat
X-Device-Token: <device-token>
Content-Type: application/json

{
  "device_id": "550e8400-...",
  "agent_version": "1.0.0",
  "system": {
    "hostname": "MacBook Pro",
    "os": "darwin",
    "uptime_seconds": 86400
  }
}

Response 200 OK:
{
  "pending_commands": [
    {
      "id": "cmd-123",
      "type": "start_feature",
      "params": {
        "name": "oauth integration"
      }
    }
  ]
}
```

#### Report State

```http
POST /agent/report_state
X-Device-Token: <device-token>
Content-Type: application/json

{
  "device_id": "550e8400-...",
  "worktrees": [
    {
      "name": "oauth-integration",
      "status": "running",
      "branch": "feature/oauth-integration",
      "url": "https://example.com"
    }
  ],
  "timestamp": 1697900000
}

Response 200 OK:
{
  "accepted": true
}
```

## Authentication

### Device Authentication

Devices authenticate using a long-lived token in request headers:

```
X-Device-Token: <device-token>
```

Token is obtained during device registration and stored in `~/.worktree/config.toml`.

### User Authentication

Web UI uses session-based authentication (Rails cookies).

API clients can use bearer tokens:

```
Authorization: Bearer <user-token>
```

## Error Handling

### gRPC Status Codes

| Code | Description | Example |
|------|-------------|---------|
| `OK` | Success | Worktree started successfully |
| `INVALID_ARGUMENT` | Invalid request parameters | Missing feature name |
| `NOT_FOUND` | Resource not found | Worktree doesn't exist |
| `ALREADY_EXISTS` | Resource already exists | Worktree name conflict |
| `PERMISSION_DENIED` | Not authorized | Invalid device token |
| `UNAVAILABLE` | Service unavailable | Control plane unreachable |
| `INTERNAL` | Internal error | Unexpected error |

### REST API Status Codes

| Code | Description | Example |
|------|-------------|---------|
| `200` | Success | List devices |
| `201` | Created | Device registered |
| `202` | Accepted | Command queued |
| `204` | No Content | Device deleted |
| `400` | Bad Request | Invalid parameters |
| `401` | Unauthorized | Invalid token |
| `403` | Forbidden | Not your device |
| `404` | Not Found | Device not found |
| `409` | Conflict | Resource already exists |
| `500` | Internal Error | Server error |

## Message Flow Examples

### Starting a Feature (Local)

```
CLI Tool                Agent               Worktree Core
   |                      |                       |
   |--StartFeature------->|                       |
   |                      |--execute------------->|
   |                      |                       |
   |                      |                       |--create worktree
   |                      |                       |--setup devcontainer
   |                      |                       |--provision tunnel
   |                      |<-------Feature--------|
   |<------Feature--------|                       |
   |                      |                       |
   |                      |--queue state update-->| SQLite
```

### Starting a Feature (Remote)

```
Web UI          Control Plane      Agent (via Tailscale)    Worktree Core
   |                  |                      |                    |
   |--POST start----->|                      |                    |
   |<--202 Accepted---|                      |                    |
   |                  |--gRPC StartFeature-->|                    |
   |                  |                      |--execute---------->|
   |                  |                      |                    |--create worktree
   |                  |                      |<----Feature--------|
   |                  |<------Feature--------|                    |
   |<--Turbo Stream---|                      |                    |
   |  (real-time)     |                      |                    |
```

### State Synchronization

```
Agent                   Control Plane           Web UI
   |                          |                    |
   |--heartbeat (30s)-------->|                    |
   |<--pending commands-------|                    |
   |                          |                    |
   |--on state change-------->|                    |
   |  (event-driven)          |--Turbo Stream----->|
   |                          |                    |
   |--drain offline queue---->|                    |
   |  (when reconnected)      |                    |
```
