//! Cloudflare API client.
//!
//! Provides minimal, blocking HTTP helpers for provisioning and cleaning up
//! Cloudflare tunnels and DNS records during feature workflow execution.

use crate::{Error, Result};
use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT},
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;

const API_BASE: &str = "https://api.cloudflare.com/client/v4";
const USER_AGENT_VALUE: &str = "branchbox/feature-workflow";

/// Cloudflare HTTP client wrapper.
#[derive(Debug, Clone)]
pub struct CloudflareClient {
    http: Client,
    account_id: String,
    api_base: String,
}

impl CloudflareClient {
    /// Build a new client using an API token and account identifier.
    pub fn new(api_token: impl Into<String>, account_id: impl Into<String>) -> Result<Self> {
        Self::with_base_url(api_token, account_id, API_BASE)
    }

    /// Build a new client with a custom API base URL (useful for testing).
    pub(crate) fn with_base_url(
        api_token: impl Into<String>,
        account_id: impl Into<String>,
        api_base: impl Into<String>,
    ) -> Result<Self> {
        let api_token = api_token.into();
        let account_id = account_id.into();
        let api_base = api_base.into();

        if api_token.trim().is_empty() {
            return Err(Error::validation("Cloudflare API token is empty"));
        }

        if account_id.trim().is_empty() {
            return Err(Error::validation("Cloudflare account id is empty"));
        }

        let mut headers = HeaderMap::new();
        let auth_value =
            HeaderValue::from_str(&format!("Bearer {}", api_token)).map_err(|err| {
                Error::validation(format!("Invalid Cloudflare API token header: {}", err))
            })?;
        headers.insert(AUTHORIZATION, auth_value);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));

        let http = Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|err| Error::other(format!("Failed to build Cloudflare client: {}", err)))?;

        Ok(Self {
            http,
            account_id,
            api_base,
        })
    }

    /// Attempt to find an active tunnel by name.
    pub fn find_tunnel_by_name(&self, name: &str) -> Result<Option<TunnelSummary>> {
        let url = format!("{}/accounts/{}/cfd_tunnel", self.api_base, self.account_id);
        let resp: ApiListResponse<TunnelSummaryInternal> =
            self.http.get(url).query(&[("name", name)]).send()?.json()?;

        let tunnels = resp.into_result()?;
        Ok(tunnels
            .into_iter()
            .find(|tunnel| tunnel.deleted_at.is_none())
            .map(TunnelSummary::from))
    }

    /// Provision a new tunnel and return its id + token.
    pub fn create_tunnel(&self, name: &str) -> Result<TunnelProvision> {
        let url = format!("{}/accounts/{}/cfd_tunnel", self.api_base, self.account_id);
        let resp: ApiResponse<TunnelCreate> = self
            .http
            .post(url)
            .json(&json!({
                "name": name,
                "config_src": "cloudflare"
            }))
            .send()?
            .json()?;

        let result = resp.into_result()?;
        let token = result
            .token
            .ok_or_else(|| Error::validation("Cloudflare API response missing tunnel token"))?;

        Ok(TunnelProvision {
            id: result.id,
            name: result.name,
            token,
        })
    }

    /// Configure the tunnel ingress routing.
    pub fn configure_tunnel(
        &self,
        tunnel_id: &str,
        hostname: &str,
        service_url: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/accounts/{}/cfd_tunnel/{}/configurations",
            self.api_base, self.account_id, tunnel_id
        );

        let payload = json!({
            "config": {
                "ingress": [
                    {
                        "hostname": hostname,
                        "service": service_url
                    },
                    {
                        "service": "http_status:404"
                    }
                ]
            }
        });

        let resp: ApiResponse<serde_json::Value> =
            self.http.put(url).json(&payload).send()?.json()?;
        resp.into_result().map(|_| ())
    }

    /// Ensure a proxied CNAME exists pointing the hostname to the tunnel id.
    pub fn ensure_cname_record(
        &self,
        hostname: &str,
        base_domain: &str,
        tunnel_id: &str,
    ) -> Result<()> {
        if let Some(zone_id) = self.lookup_zone_id(base_domain)? {
            let target = format!("{tunnel_id}.cfargotunnel.com");
            if let Some(existing) = self.get_dns_record(&zone_id, hostname)? {
                if existing.content == target {
                    return Ok(());
                }

                self.update_dns_record(&zone_id, &existing.id, hostname, &target)?;
            } else {
                self.create_dns_record(&zone_id, hostname, &target)?;
            }
        } else {
            return Err(Error::validation(format!(
                "Cloudflare zone not found for {}",
                base_domain
            )));
        }

        Ok(())
    }

    /// Delete a tunnel by id.
    pub fn delete_tunnel(&self, tunnel_id: &str) -> Result<()> {
        let url = format!(
            "{}/accounts/{}/cfd_tunnel/{}",
            self.api_base, self.account_id, tunnel_id
        );
        let resp: ApiResponse<serde_json::Value> = self.http.delete(url).send()?.json()?;
        resp.into_result().map(|_| ())
    }

    /// Delete DNS records matching the hostname. Returns true if a record was deleted.
    pub fn delete_dns_record(&self, hostname: &str, base_domain: &str) -> Result<bool> {
        let Some(zone_id) = self.lookup_zone_id(base_domain)? else {
            return Ok(false);
        };

        let records = self.get_dns_records(&zone_id, hostname)?;
        let mut deleted = false;

        for record in records {
            let url = format!(
                "{}/zones/{}/dns_records/{}",
                self.api_base, zone_id, record.id
            );
            let resp: ApiResponse<serde_json::Value> = self.http.delete(url).send()?.json()?;
            resp.into_result()?;
            deleted = true;
        }

        Ok(deleted)
    }

    fn lookup_zone_id(&self, base_domain: &str) -> Result<Option<String>> {
        let url = format!("{}/zones", self.api_base);
        let resp: ApiListResponse<ZoneResult> = self
            .http
            .get(url)
            .query(&[("name", base_domain)])
            .send()?
            .json()?;

        let zones = resp.into_result()?;
        Ok(zones.into_iter().next().map(|zone| zone.id))
    }

    fn get_dns_records(&self, zone_id: &str, hostname: &str) -> Result<Vec<DnsRecord>> {
        let url = format!("{}/zones/{}/dns_records", self.api_base, zone_id);
        let resp: ApiListResponse<DnsRecord> = self
            .http
            .get(url)
            .query(&[("name", hostname)])
            .send()?
            .json()?;

        resp.into_result()
    }

    fn get_dns_record(&self, zone_id: &str, hostname: &str) -> Result<Option<DnsRecord>> {
        let mut records = self.get_dns_records(zone_id, hostname)?;
        Ok(records.pop())
    }

    fn create_dns_record(&self, zone_id: &str, hostname: &str, target: &str) -> Result<()> {
        let url = format!("{}/zones/{}/dns_records", self.api_base, zone_id);
        let payload = json!({
            "type": "CNAME",
            "name": hostname,
            "content": target,
            "proxied": true
        });

        let resp: ApiResponse<DnsRecord> = self.http.post(url).json(&payload).send()?.json()?;
        resp.into_result().map(|_| ())
    }

    fn update_dns_record(
        &self,
        zone_id: &str,
        record_id: &str,
        hostname: &str,
        target: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/zones/{}/dns_records/{}",
            self.api_base, zone_id, record_id
        );
        let payload = json!({
            "type": "CNAME",
            "name": hostname,
            "content": target,
            "proxied": true
        });

        let resp: ApiResponse<DnsRecord> = self.http.put(url).json(&payload).send()?.json()?;
        resp.into_result().map(|_| ())
    }
}

/// Provisioned tunnel information.
#[derive(Debug, Clone)]
pub struct TunnelProvision {
    pub id: String,
    pub name: String,
    pub token: String,
}

/// Lightweight tunnel summary.
#[derive(Debug, Clone)]
pub struct TunnelSummary {
    pub id: String,
    pub name: String,
}

impl From<TunnelSummaryInternal> for TunnelSummary {
    fn from(value: TunnelSummaryInternal) -> Self {
        Self {
            id: value.id,
            name: value.name,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    success: bool,
    errors: Vec<ApiError>,
    #[serde(bound(deserialize = "T: DeserializeOwned"))]
    result: T,
}

#[derive(Debug, Deserialize)]
struct ApiListResponse<T> {
    success: bool,
    errors: Vec<ApiError>,
    #[serde(bound(deserialize = "T: DeserializeOwned"))]
    result: Vec<T>,
}

impl<T> ApiResponse<T> {
    fn into_result(self) -> Result<T> {
        if self.success {
            Ok(self.result)
        } else {
            Err(Error::validation(format_api_errors(self.errors)))
        }
    }
}

impl<T> ApiListResponse<T> {
    fn into_result(self) -> Result<Vec<T>> {
        if self.success {
            Ok(self.result)
        } else {
            Err(Error::validation(format_api_errors(self.errors)))
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(default)]
    code: Option<i64>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct TunnelSummaryInternal {
    id: String,
    name: String,
    #[serde(default)]
    deleted_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TunnelCreate {
    id: String,
    name: String,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ZoneResult {
    id: String,
}

#[derive(Debug, Deserialize, Clone)]
struct DnsRecord {
    id: String,
    content: String,
}

fn format_api_errors(errors: Vec<ApiError>) -> String {
    if errors.is_empty() {
        "Cloudflare API error (unknown)".to_string()
    } else {
        errors
            .into_iter()
            .map(|error| match error.code {
                Some(code) => format!("[{}] {}", code, error.message),
                None => error.message,
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_errors_handles_empty() {
        assert_eq!(
            format_api_errors(Vec::new()),
            "Cloudflare API error (unknown)"
        );
    }

    #[test]
    fn format_errors_combines_entries() {
        let errors = vec![
            ApiError {
                code: Some(101),
                message: "Invalid token".into(),
            },
            ApiError {
                code: None,
                message: "Another issue".into(),
            },
        ];

        let formatted = format_api_errors(errors);
        assert!(formatted.contains("Invalid token"));
        assert!(formatted.contains("[101]"));
        assert!(formatted.contains("Another issue"));
    }

    #[test]
    fn tunnel_create_response_parses_token() {
        let payload = r#"
        {
            "success": true,
            "errors": [],
            "result": {
                "id": "1234-5678",
                "name": "branchbox-test",
                "token": "secret-token"
            }
        }"#;

        let resp: ApiResponse<TunnelCreate> = serde_json::from_str(payload).unwrap();
        let result = resp.into_result().unwrap();
        assert_eq!(result.id, "1234-5678");
        assert_eq!(result.token.unwrap(), "secret-token");
    }

    #[test]
    fn test_client_new_empty_token() {
        let result = CloudflareClient::new("", "account123");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("API token is empty"));
    }

    #[test]
    fn test_client_new_empty_account() {
        let result = CloudflareClient::new("token123", "");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("account id is empty"));
    }

    #[test]
    fn test_client_new_valid() {
        let result = CloudflareClient::new("test_token", "test_account");
        assert!(result.is_ok());
    }

    #[test]
    fn test_find_tunnel_by_name() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/accounts/test-account/cfd_tunnel")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "test-tunnel".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": [{
                    "id": "tunnel-123",
                    "name": "test-tunnel",
                    "deleted_at": null
                }]
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.find_tunnel_by_name("test-tunnel").unwrap();

        assert!(result.is_some());
        let tunnel = result.unwrap();
        assert_eq!(tunnel.id, "tunnel-123");
        assert_eq!(tunnel.name, "test-tunnel");
        mock.assert();
    }

    #[test]
    fn test_find_tunnel_by_name_not_found() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/accounts/test-account/cfd_tunnel")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "missing-tunnel".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": []
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.find_tunnel_by_name("missing-tunnel").unwrap();

        assert!(result.is_none());
        mock.assert();
    }

    #[test]
    fn test_find_tunnel_filters_deleted() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/accounts/test-account/cfd_tunnel")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "test-tunnel".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": [{
                    "id": "tunnel-123",
                    "name": "test-tunnel",
                    "deleted_at": "2024-01-01T00:00:00Z"
                }]
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.find_tunnel_by_name("test-tunnel").unwrap();

        assert!(result.is_none());
        mock.assert();
    }

    #[test]
    fn test_create_tunnel() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/accounts/test-account/cfd_tunnel")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": {
                    "id": "new-tunnel-123",
                    "name": "my-tunnel",
                    "token": "tunnel-token-xyz"
                }
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.create_tunnel("my-tunnel").unwrap();

        assert_eq!(result.id, "new-tunnel-123");
        assert_eq!(result.name, "my-tunnel");
        assert_eq!(result.token, "tunnel-token-xyz");
        mock.assert();
    }

    #[test]
    fn test_create_tunnel_no_token() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/accounts/test-account/cfd_tunnel")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": {
                    "id": "new-tunnel-123",
                    "name": "my-tunnel"
                }
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.create_tunnel("my-tunnel");

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("missing tunnel token"));
        mock.assert();
    }

    #[test]
    fn test_configure_tunnel() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock(
                "PUT",
                "/accounts/test-account/cfd_tunnel/tunnel-123/configurations",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": {}
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result =
            client.configure_tunnel("tunnel-123", "test.example.com", "http://localhost:3000");

        assert!(result.is_ok());
        mock.assert();
    }

    #[test]
    fn test_delete_tunnel() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("DELETE", "/accounts/test-account/cfd_tunnel/tunnel-123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": {}
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.delete_tunnel("tunnel-123");

        assert!(result.is_ok());
        mock.assert();
    }

    #[test]
    fn test_api_error_response() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/accounts/test-account/cfd_tunnel")
            .match_query(mockito::Matcher::UrlEncoded("name".into(), "test".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": false,
                "errors": [
                    {"code": 1003, "message": "Invalid request"},
                    {"message": "Another error"}
                ],
                "result": []
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.find_tunnel_by_name("test");

        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(err_str.contains("Invalid request"));
        assert!(err_str.contains("[1003]"));
        assert!(err_str.contains("Another error"));
        mock.assert();
    }

    #[test]
    fn test_ensure_cname_record_zone_not_found() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/zones")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "example.com".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": []
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.ensure_cname_record("test.example.com", "example.com", "tunnel-123");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("zone not found"));
        mock.assert();
    }

    #[test]
    fn test_ensure_cname_record_creates_new() {
        let mut server = mockito::Server::new();

        let zone_mock = server
            .mock("GET", "/zones")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "example.com".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": [{"id": "zone-123"}]
            }"#,
            )
            .create();

        let dns_get_mock = server
            .mock("GET", "/zones/zone-123/dns_records")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "test.example.com".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": []
            }"#,
            )
            .create();

        let dns_create_mock = server
            .mock("POST", "/zones/zone-123/dns_records")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": {"id": "record-123", "content": "tunnel-123.cfargotunnel.com"}
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.ensure_cname_record("test.example.com", "example.com", "tunnel-123");

        assert!(result.is_ok());
        zone_mock.assert();
        dns_get_mock.assert();
        dns_create_mock.assert();
    }

    #[test]
    fn test_ensure_cname_record_updates_existing() {
        let mut server = mockito::Server::new();

        let zone_mock = server
            .mock("GET", "/zones")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "example.com".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": [{"id": "zone-123"}]
            }"#,
            )
            .create();

        let dns_get_mock = server
            .mock("GET", "/zones/zone-123/dns_records")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "test.example.com".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": [{"id": "record-123", "content": "old-tunnel.cfargotunnel.com"}]
            }"#,
            )
            .create();

        let dns_update_mock = server
            .mock("PUT", "/zones/zone-123/dns_records/record-123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": {"id": "record-123", "content": "tunnel-123.cfargotunnel.com"}
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.ensure_cname_record("test.example.com", "example.com", "tunnel-123");

        assert!(result.is_ok());
        zone_mock.assert();
        dns_get_mock.assert();
        dns_update_mock.assert();
    }

    #[test]
    fn test_ensure_cname_record_skips_matching() {
        let mut server = mockito::Server::new();

        let zone_mock = server
            .mock("GET", "/zones")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "example.com".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": [{"id": "zone-123"}]
            }"#,
            )
            .create();

        let dns_get_mock = server
            .mock("GET", "/zones/zone-123/dns_records")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "test.example.com".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": [{"id": "record-123", "content": "tunnel-123.cfargotunnel.com"}]
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.ensure_cname_record("test.example.com", "example.com", "tunnel-123");

        assert!(result.is_ok());
        zone_mock.assert();
        dns_get_mock.assert();
    }

    #[test]
    fn test_delete_dns_record() {
        let mut server = mockito::Server::new();

        let zone_mock = server
            .mock("GET", "/zones")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "example.com".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": [{"id": "zone-123"}]
            }"#,
            )
            .create();

        let dns_get_mock = server
            .mock("GET", "/zones/zone-123/dns_records")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "test.example.com".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": [{"id": "record-123", "content": "tunnel-123.cfargotunnel.com"}]
            }"#,
            )
            .create();

        let dns_delete_mock = server
            .mock("DELETE", "/zones/zone-123/dns_records/record-123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": {}
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.delete_dns_record("test.example.com", "example.com");

        assert!(result.is_ok());
        assert!(result.unwrap());
        zone_mock.assert();
        dns_get_mock.assert();
        dns_delete_mock.assert();
    }

    #[test]
    fn test_delete_dns_record_not_found() {
        let mut server = mockito::Server::new();

        let zone_mock = server
            .mock("GET", "/zones")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "example.com".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "success": true,
                "errors": [],
                "result": []
            }"#,
            )
            .create();

        let client =
            CloudflareClient::with_base_url("test_token", "test-account", server.url()).unwrap();
        let result = client.delete_dns_record("test.example.com", "example.com");

        assert!(result.is_ok());
        assert!(!result.unwrap());
        zone_mock.assert();
    }
}
