use niuma_core::remote::login_flow::{
    DesktopLoginBindingResult, DesktopLoginStartRequest, DesktopLoginStartResponse,
};

pub trait BrowserOpener {
    fn open_url(&self, url: &str) -> Result<(), String>;
}

pub struct RemoteLoginFlow;

impl RemoteLoginFlow {
    pub fn build_start_request(
        device_name: &str,
        device_fingerprint: String,
        desktop_public_key: String,
        device_identity_public_key: String,
    ) -> DesktopLoginStartRequest {
        DesktopLoginStartRequest::new(
            device_name,
            device_fingerprint,
            desktop_public_key,
            device_identity_public_key,
        )
    }

    pub fn open_login_url(
        opener: &dyn BrowserOpener,
        response: &DesktopLoginStartResponse,
    ) -> Result<(), String> {
        opener.open_url(&response.login_url)
    }

    pub fn apply_binding_result(result: DesktopLoginBindingResult) -> DesktopLoginBindingResult {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBrowserOpener;

    impl BrowserOpener for TestBrowserOpener {
        fn open_url(&self, url: &str) -> Result<(), String> {
            assert_eq!(url, "https://remote.example.com/login");
            Ok(())
        }
    }

    #[test]
    fn builds_start_request_and_opens_login_url() {
        let request = RemoteLoginFlow::build_start_request(
            "NiuMa MacBook",
            "f".repeat(64),
            "desktop-public-key".to_string(),
            "{\"kty\":\"EC\"}".to_string(),
        );
        assert!(request.supports_remote_control());

        let response = DesktopLoginStartResponse {
            request_id: "login_req_1".to_string(),
            poll_token: "poll_token_1".to_string(),
            login_url: "https://remote.example.com/login".to_string(),
            expires_in: 300,
        };
        RemoteLoginFlow::open_login_url(&TestBrowserOpener, &response).unwrap();
    }

    #[test]
    fn returns_binding_result_without_persisting_credentials() {
        let result = DesktopLoginBindingResult {
            user_id: "user_1".to_string(),
            user_email: "user@example.com".to_string(),
            user_role: "owner".to_string(),
            device_id: "dev_1".to_string(),
            device_name: "NiuMa MacBook".to_string(),
            device_token: "device-token".to_string(),
        };

        assert_eq!(
            RemoteLoginFlow::apply_binding_result(result).device_id,
            "dev_1"
        );
    }
}
