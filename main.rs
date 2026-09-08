//! Permite `cargo run` e é o executável `mcp-tools`: sobe o servidor via stdio.

fn main() {
    std::process::exit(mcp_tools::server::main());
}
