//rustc version = rustc 1.91.1 (ed61e7d7e 2025-11-07)
//cargo version = cargo 1.91.1 (ea2d97820 2025-10-10)

//adicionei dependencia no arquivo Cargo.toml: tokio = { version = "1", features = ["full"] }

// terminal precisa estar na pasta chat-server para rodar esse código
// para rodar o servidor, use o comando: cargo run
// para testar o servidor, abra outro terminal, você pode usar o comando: telnet (precia ser instalado) ou netcat (nc)

use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() {
    println!("Iniciando o servidor de chat...");

    //127.0.0.1 é o localhost
    //porta 8080
    let listener = TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap();
    println!("Servidor rodando em 127.0.0.1:8080");

    loop {
        //quando alguém conecta, captura o socket e o endereço
        let (mut socket, addr) = listener.accept().await.unwrap();
        println!("Novo cliente conectado de: {}", addr);

        //cria uma thread assíncrona para lidar com o cliente
        tokio::spawn(async move {
            //buffer para receber dados do cliente
            let mut buffer = vec![0u8; 1024];

            loop {
                //lê os dados enviados pelo cliente
                let readed_bytes = match socket.read(&mut buffer).await {
                    Ok(0) => {
                        println!("Cliente {} desconectou", addr);
                        return;
                    }
                    Ok(n) => n,
                    Err(erro) => {
                        eprintln!("Erro ao ler dados do cliente {}: {}", addr, erro);
                        return;
                    }
                };
                
                //envia os dados de volta para o cliente (eco)
                if socket.write_all(&buffer[..readed_bytes]).await.is_err() {
                    println!("Erro ao enviar dados para o cliente {}", addr);
                    return;
                }
            }
        });
    }
}
