//! Raiz dos testes. Espelha a árvore de `core/` e `domains/`, como o `tests/` da
//! versão Python: um módulo por arquivo testado.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod conftest;
mod helpers;

mod core;
mod domains;
mod test_server;
