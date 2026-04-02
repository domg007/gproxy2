use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use std::path::Path;

/// Build the standard CA certificate parameters.
fn ca_params() -> CertificateParams {
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "gproxy-recorder CA");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "gproxy");
    params.key_usages.push(KeyUsagePurpose::KeyCertSign);
    params.key_usages.push(KeyUsagePurpose::CrlSign);
    params
}

/// Generate a self-signed CA certificate and private key.
/// Returns (cert_pem, key_pem).
pub fn generate_ca() -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let params = ca_params();
    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    Ok((cert_pem, key_pem))
}

/// A loaded CA certificate and its key pair, ready to sign leaf certs.
pub struct CaAuthority {
    pub ca_key: KeyPair,
    pub ca_cert: rcgen::Certificate,
}

/// Load a CA certificate and private key from a PEM file.
///
/// The file should contain both the certificate and the private key in PEM
/// format (as produced by `generate_ca`). Because rcgen 0.13 does not expose
/// `CertificateParams::from_ca_cert_der` without the `x509-parser` feature, we
/// re-create the CA parameters deterministically and re-self-sign with the
/// loaded key. The resulting `Certificate` object is functionally identical to
/// the original (same key, same DN) and can sign leaf certs.
pub fn load_ca(path: &Path) -> Result<CaAuthority, Box<dyn std::error::Error + Send + Sync>> {
    let pem_str = std::fs::read_to_string(path)?;

    // Extract the private key PEM section
    let ca_key = KeyPair::from_pem(&pem_str)?;

    // Re-create the same CA params and self-sign with the loaded key
    let params = ca_params();
    let ca_cert = params.self_signed(&ca_key)?;

    Ok(CaAuthority { ca_key, ca_cert })
}

/// Issue a leaf certificate for a domain, signed by the CA.
/// Returns (cert_pem, key_pem).
pub fn issue_cert(
    ca: &CaAuthority,
    domain: &str,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, domain);
    params.subject_alt_names = vec![SanType::DnsName(domain.try_into()?)];
    params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);

    let leaf_key = KeyPair::generate()?;
    let leaf_cert = params.signed_by(&leaf_key, &ca.ca_cert, &ca.ca_key)?;

    Ok((leaf_cert.pem(), leaf_key.serialize_pem()))
}
