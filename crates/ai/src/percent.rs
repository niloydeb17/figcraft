//! Percent-encoding for URL queries and form bodies (RFC 3986 unreserved characters pass).

/// `s` with everything but `A-Z a-z 0-9 - . _ ~` written as `%XX`.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `s` with `%XX` and `+` undone; a malformed escape stays as it is.
pub fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b),
                }
            }
            _ => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `a=1&b=2` as `(a, 1)`, `(b, 2)`, both sides decoded.
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

/// `(a, 1)`, `(b, 2)` as `a=1&b=2`, encoded.
pub fn form(fields: &[(&str, &str)]) -> String {
    fields.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreserved_pass_and_the_rest_is_escaped() {
        assert_eq!(encode("abc-._~123"), "abc-._~123");
        assert_eq!(encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }

    #[test]
    fn decoding_undoes_encoding_and_tolerates_junk() {
        let s = "http://127.0.0.1:1455/auth/callback?x=1 é";
        assert_eq!(decode(&encode(s)), s);
        assert_eq!(decode("a+b%2"), "a b%2");
        assert_eq!(decode("%zz"), "%zz");
    }

    #[test]
    fn queries_parse_to_decoded_pairs() {
        let q = parse_query("code=abc%2Fdef&state=s1&flag&empty=");
        assert_eq!(
            q,
            [("code".into(), "abc/def".into()), ("state".into(), "s1".into()), ("flag".into(), String::new()), ("empty".into(), String::new())]
        );
        assert_eq!(form(&[("a", "x y"), ("b", "1&2")]), "a=x%20y&b=1%262");
    }
}
