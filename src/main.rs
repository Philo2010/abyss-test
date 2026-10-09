use crate::metro::server::Server;

mod package;
mod metro;


#[tokio::main]
async fn main() {
    let server = Server::new();
    if let Err(e) = server.listen("127.0.0.1:8001").await {
        eprintln!("Server stopped: {e}");
    }
}
