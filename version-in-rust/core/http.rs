//! Cliente HTTP compartilhado por quem fala com a internet (`binaries`,
//! `freesound`, `downloader`).
//!
//! Usa rustls embutido (nada de OpenSSL do sistema) e valida certificados com
//! a cadeia de confiança do sistema operacional, o que faz o download funcionar
//! também atrás de proxies corporativos que reassinam TLS. Proxy vem das
//! variáveis `HTTPS_PROXY`/`HTTP_PROXY`, como em qualquer cliente.

use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig};
use ureq::Agent;

/// Agente HTTP com timeout global e `User-Agent` fixos.
pub fn agent(timeout: Duration, user_agent: &str) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(user_agent)
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent()
}
