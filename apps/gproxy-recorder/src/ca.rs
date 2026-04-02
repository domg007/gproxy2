use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use std::path::Path;

const CA_KEY_FILE_EXT: &str = ".key.der";

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

/// Generate a self-signed CA certificate.
/// Writes cert PEM to `out_path` and key DER to `out_path + ".key.der"`.
pub fn generate_ca(out_path: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let params = ca_params();
    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    // Write cert PEM
    std::fs::write(out_path, cert.pem())?;

    // Write key DER (binary, roundtrips reliably with ring)
    let key_path = format!("{}{}", out_path.display(), CA_KEY_FILE_EXT);
    std::fs::write(&key_path, key_pair.serialize_der())?;

    Ok(())
}

/// A loaded CA certificate and its key pair, ready to sign leaf certs.
pub struct CaAuthority {
    pub ca_key: KeyPair,
    pub ca_cert: rcgen::Certificate,
}

/// Load a CA certificate and private key.
/// Reads cert PEM from `path` and key DER from `path + ".key.der"`.
pub fn load_ca(path: &Path) -> Result<CaAuthority, Box<dyn std::error::Error + Send + Sync>> {
    let key_path = format!("{}{}", path.display(), CA_KEY_FILE_EXT);
    let key_der = std::fs::read(&key_path)
        .map_err(|e| format!("Failed to read key file {}: {}", key_path, e))?;

    let ca_key = KeyPair::try_from(key_der.as_slice())?;

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
