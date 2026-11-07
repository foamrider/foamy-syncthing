//! TLS for the Syncthing GUI connection (Syncshell `targetTLSConfig`):
//!
//! - **pinned**: the GUI certificate next to `config.xml` (or
//!   `--tls-certificate`). The peer's end-entity certificate must be byte
//!   for byte that certificate (constant-time compare; Syncshell compared
//!   SHA-256 digests, which is the same test), and the
//!   pinned certificate must be within its validity period. Host names are
//!   not checked (the pin replaces public-PKI verification);
//! - **insecure**: any certificate, only for an explicit HTTPS target with
//!   `--insecure-tls`;
//! - **system roots**: an explicit HTTPS target without a certificate:
//!   ordinary WebPKI verification against the system's CA store.
//!
//! In every mode the handshake signatures are verified against the peer's
//! key, TLS 1.2 is the minimum, and session resumption is off, so every
//! connection is verified afresh. The crypto provider, the CPU check and
//! the system CA store are the kit's (`foamy_helper::tls`: Graviola, pure
//! Rust and assembly). Graviola needs AES, carry-less multiplication, AVX2,
//! BMI2 and ADX on x86-64, or the crypto extensions on ARMv8; the transport
//! checks them with `foamy_helper::tls::cpu_supported` before any handshake
//! and otherwise reports `foamy_helper::tls::UNSUPPORTED`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, SignatureScheme};

/// How the server certificate is checked.
pub enum Mode {
    /// The pinned DER certificate and its validity (Unix seconds).
    Pinned {
        der: Vec<u8>,
        not_before: i64,
        not_after: i64,
    },
    Insecure,
    SystemRoots,
}

/// A client configuration for one target. Errors are for the caller's log
/// only (they never contain secrets).
pub fn client_config(mode: Mode) -> Result<Arc<ClientConfig>, String> {
    let provider = foamy_helper::tls::provider();
    let algorithms = provider.signature_verification_algorithms;
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(rustls::ALL_VERSIONS)
        .map_err(|error| error.to_string())?;
    let mut config = match mode {
        Mode::Pinned {
            der,
            not_before,
            not_after,
        } => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pinned {
                der,
                not_before,
                not_after,
                algorithms,
            }))
            .with_no_client_auth(),
        Mode::Insecure => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyCertificate { algorithms }))
            .with_no_client_auth(),
        Mode::SystemRoots => {
            let roots = foamy_helper::tls::system_roots().map_err(|error| error.to_string())?;
            let verifier =
                rustls::client::WebPkiServerVerifier::builder_with_provider(roots, provider)
                    .build()
                    .map_err(|error| error.to_string())?;
            builder.with_webpki_verifier(verifier).with_no_client_auth()
        }
    };
    config.resumption = rustls::client::Resumption::disabled();
    Ok(Arc::new(config))
}

/// The server name rustls sends and checks: an IP address (no SNI) or a
/// DNS name; anything else falls back to `localhost`, which only the
/// system-roots mode would check.
pub fn server_name(host: &str) -> ServerName<'static> {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    ServerName::try_from(host.to_string())
        .unwrap_or_else(|_| ServerName::try_from("localhost").expect("valid name"))
}

#[derive(Debug)]
struct Pinned {
    der: Vec<u8>,
    not_before: i64,
    not_after: i64,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if !constant_time_eq(end_entity.as_ref(), &self.der) {
            return Err(Error::General("peer certificate mismatch".into()));
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        if now < self.not_before || now > self.not_after {
            return Err(Error::General(
                "peer certificate is outside its validity period".into(),
            ));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[derive(Debug)]
struct AnyCertificate {
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (x, y) in a.iter().zip(b) {
        difference |= x ^ y;
    }
    // Keep the comparison from being short-circuited.
    std::hint::black_box(difference) == 0
}

/// The pin for a PEM certificate file's contents: Go's `pem.Decode` (the
/// first block, which must be a `CERTIFICATE`), then the certificate's
/// validity period. `None` when it is not a usable certificate.
pub fn pin_from_pem(contents: &[u8]) -> Option<Mode> {
    let (label, der) = first_pem_block(contents)?;
    if label != "CERTIFICATE" {
        return None;
    }
    let (not_before, not_after) = validity(&der)?;
    Some(Mode::Pinned {
        der,
        not_before,
        not_after,
    })
}

fn first_pem_block(contents: &[u8]) -> Option<(String, Vec<u8>)> {
    let text = std::str::from_utf8(contents).ok()?;
    let mut rest = text;
    loop {
        let start = rest.find("-----BEGIN ")?;
        // A block starts at the beginning of a line.
        if start > 0 && !rest[..start].ends_with('\n') {
            rest = &rest[start + 11..];
            continue;
        }
        let after = &rest[start + 11..];
        let label_end = after.find("-----")?;
        let label = &after[..label_end];
        let body_start = &after[label_end + 5..];
        let end_marker = format!("-----END {label}-----");
        let Some(end) = body_start.find(&end_marker) else {
            rest = body_start;
            continue;
        };
        let mut body = &body_start[..end];
        // Optional RFC 1421 headers end at the first empty line.
        if let Some(colon) = body.find(':')
            && body[..colon].lines().count() <= 2
            && let Some(blank) = body.find("\n\n")
        {
            body = &body[blank + 2..];
        }
        let encoded: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        match base64_decode(&encoded) {
            Some(der) => return Some((label.to_string(), der)),
            None => rest = &body_start[end + end_marker.len()..],
        }
    }
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        Some(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let last = index == bytes.len() / 4 - 1;
        let padding = chunk.iter().rev().take_while(|&&b| b == b'=').count();
        if padding > 2 || (padding > 0 && !last) {
            return None;
        }
        let mut word = 0u32;
        for &byte in &chunk[..4 - padding] {
            word = (word << 6) | value(byte)?;
        }
        word <<= 6 * padding as u32;
        let decoded = [(word >> 16) as u8, (word >> 8) as u8, word as u8];
        out.extend_from_slice(&decoded[..3 - padding]);
    }
    Some(out)
}

/// One DER element: (tag, contents, rest).
fn der(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (first as usize, rest)
    } else {
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let length = rest[..count]
            .iter()
            .fold(0usize, |acc, &b| (acc << 8) | b as usize);
        (length, &rest[count..])
    };
    if rest.len() < length {
        return None;
    }
    Some((tag, &rest[..length], &rest[length..]))
}

/// `(notBefore, notAfter)` of a DER X.509 certificate, in Unix seconds.
pub fn validity(certificate: &[u8]) -> Option<(i64, i64)> {
    let (0x30, body, trailing) = der(certificate)? else {
        return None;
    };
    if !trailing.is_empty() {
        return None;
    }
    let (0x30, tbs, _) = der(body)? else {
        return None;
    };
    let mut rest = tbs;
    let (tag, _, after) = der(rest)?;
    if tag == 0xa0 {
        rest = after;
    }
    let (0x02, _, rest) = der(rest)? else {
        return None;
    };
    let (0x30, _, rest) = der(rest)? else {
        return None;
    };
    let (0x30, _, rest) = der(rest)? else {
        return None;
    };
    let (0x30, period, _) = der(rest)? else {
        return None;
    };
    let (first_tag, first, rest) = der(period)?;
    let (second_tag, second, _) = der(rest)?;
    Some((asn1_time(first_tag, first)?, asn1_time(second_tag, second)?))
}

fn asn1_time(tag: u8, value: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(value).ok()?;
    let text = text.strip_suffix('Z')?;
    let digits = |s: &str| -> Option<i64> {
        if s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };
    let (year, rest) = match tag {
        0x17 if text.len() == 12 => {
            let year = digits(&text[..2])?;
            (
                if year >= 50 { 1900 + year } else { 2000 + year },
                &text[2..],
            )
        }
        0x18 if text.len() == 14 => (digits(&text[..4])?, &text[4..]),
        _ => return None,
    };
    let field = |i: usize| digits(&rest[i..i + 2]);
    let (month, day, hour, minute, second) =
        (field(0)?, field(2)?, field(4)?, field(6)?, field(8)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second)
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway self-signed P-256 certificate (CN=test, 2026-01-01 to 2036-01-01).
    const PEM: &str = "-----BEGIN CERTIFICATE-----
MIIBcjCCARmgAwIBAgIUJkmw0smvRbE1fLWNwzWOxsIYlZYwCgYIKoZIzj0EAwIw
DzENMAsGA1UEAwwEdGVzdDAeFw0yNjAxMDEwMDAwMDBaFw0zNjAxMDEwMDAwMDBa
MA8xDTALBgNVBAMMBHRlc3QwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAAQ2Ac6H
GAzxbXPUWyR9Iim2Tumb/rlZac+dvydMJrHDl87jjYQqBWgEuNJiN8gqlCgfuduq
cXE6/4MAk/yKm4Kso1MwUTAdBgNVHQ4EFgQU79pGkSc6trjkKWflaYi4HW/As6Qw
HwYDVR0jBBgwFoAU79pGkSc6trjkKWflaYi4HW/As6QwDwYDVR0TAQH/BAUwAwEB
/zAKBggqhkjOPQQDAgNHADBEAiBA2ew0KU5Q87ZZ4c1KzfFqF2gsKutXyq6Qf2ub
srYlEAIgY2q+UoK5cjyiXvgaucRX1+8WaGkkshF6zv9zL1W3beI=
-----END CERTIFICATE-----
";

    #[test]
    fn validity_comes_from_the_certificate() {
        let (_, der) = first_pem_block(PEM.as_bytes()).unwrap();
        assert_eq!(validity(&der), Some((1767225600, 2082758400)));
        let Some(Mode::Pinned {
            not_before,
            not_after,
            ..
        }) = pin_from_pem(PEM.as_bytes())
        else {
            panic!("not a pin");
        };
        assert_eq!((not_before, not_after), (1767225600, 2082758400));
    }

    #[test]
    fn only_a_leading_certificate_block_pins() {
        let key_first =
            format!("-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n{PEM}");
        assert!(pin_from_pem(key_first.as_bytes()).is_none());
        assert!(pin_from_pem(b"not pem").is_none());
        assert!(
            pin_from_pem(b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n")
                .is_none()
        );
        let preamble = format!("subject=CN=test\n{PEM}");
        assert!(pin_from_pem(preamble.as_bytes()).is_some());
    }

    #[test]
    fn base64_and_times() {
        assert_eq!(base64_decode("aGk=").unwrap(), b"hi");
        assert_eq!(base64_decode("aGlh").unwrap(), b"hia");
        assert!(base64_decode("aG=k").is_none());
        assert!(base64_decode("a").is_none());
        assert_eq!(asn1_time(0x17, b"700101000000Z"), Some(0));
        assert_eq!(asn1_time(0x18, b"20000301000000Z"), Some(951868800));
        assert_eq!(asn1_time(0x17, b"491231235959Z"), Some(2524607999));
        assert_eq!(asn1_time(0x17, b"7001010000Z"), None);
    }

    #[test]
    fn comparison_is_exact() {
        let a = [7u8; 32];
        let mut b = a;
        assert!(constant_time_eq(&a, &b));
        b[31] ^= 1;
        assert!(!constant_time_eq(&a, &b));
        assert!(!constant_time_eq(&a, &a[..31]));
    }
}
