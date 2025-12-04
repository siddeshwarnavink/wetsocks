///
/// HTTP header parser
///
/// Reference: <https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers>
///
pub mod header {
    use std::collections::HashMap;

    #[derive(Clone)]
    pub enum HttpVerb {
        Get,
        Post,
    }

    pub enum HttpParseError {
        InvalidInput,
        UnsupportedVerb,
    }

    pub struct HttpHeader {
        pub verb: HttpVerb,
        pub path: String,
        pub table: HashMap<String, String>,
    }

    pub fn parse(data: &str) -> Result<HttpHeader, HttpParseError> {
        let mut table: HashMap<String, String> = HashMap::new();

        let mut lines = data.split("\r\n");

        let request_line = lines.next().ok_or(HttpParseError::InvalidInput)?;

        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("").to_string();

        let verb = match method {
            "GET" => HttpVerb::Get,
            "POST" => HttpVerb::Post,
            _ => {
                return Err(HttpParseError::UnsupportedVerb);
            }
        };

        for line in lines {
            if line.is_empty() {
                break;
            }

            if let Some((key, value)) = line.split_once(": ") {
                table.insert(key.to_string(), value.to_string());
            }
        }

        Ok(HttpHeader { verb, path, table })
    }
}
