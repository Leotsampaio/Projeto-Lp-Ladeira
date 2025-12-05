// src/bin/cliente.rs

use tokio::net::TcpStream;
// CORREÇÃO 1: Removi o AsyncReadExt que não estava sendo usado
use tokio::io::{AsyncWriteExt, AsyncBufReadExt, BufReader};

#[tokio::main]
async fn main() {
    let ip = "127.0.0.1:8080";
    
    println!("Tentando conectar em {}...", ip);
    
    match TcpStream::connect(ip).await {
        // CORREÇÃO 2: Adicionei 'mut' antes de socket.
        // O .split() lá embaixo exige que o socket seja mutável.
        Ok(mut socket) => {
            println!("Conectado! Pode digitar suas mensagens:");
            
            let (reader, mut writer) = socket.split();
            let mut reader = BufReader::new(reader);
            
            let mut stdin = BufReader::new(tokio::io::stdin());
            
            let mut linha_do_servidor = String::new();
            let mut linha_do_usuario = String::new();

            loop {
                tokio::select! {
                    res = reader.read_line(&mut linha_do_servidor) => {
                        if res.unwrap() == 0 { 
                            println!("Servidor desconectou.");
                            break; 
                        }
                        print!("{}", linha_do_servidor);
                        linha_do_servidor.clear();
                    }

                    res = stdin.read_line(&mut linha_do_usuario) => {
                        if res.unwrap() == 0 { break; } 
                        writer.write_all(linha_do_usuario.as_bytes()).await.unwrap();
                        linha_do_usuario.clear();
                    }
                }
            }
        }
        Err(e) => {
            println!("Falha ao conectar: {}", e);
        }
    }
}