// src/conexao.rs

use tokio::net::TcpListener;
// CORREÇÃO AQUI: Trocamos AsyncReadExt por AsyncBufReadExt
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;

pub async fn iniciar() {
    let (tx, _rx) = broadcast::channel(100);

    let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();
    
    println!("--- Servidor de Chat Iniciado ---");
    println!("Rodando em 0.0.0.0:8080");
    let mut id_contador = 0;
    loop {
        
        let (mut socket, addr) = listener.accept().await.unwrap();

        // 2. Incrementamos o contador para o novo cliente
        id_contador += 1;
        
        // Vamos capturar o valor atual numa variável imutável apenas por clareza,
        // mas você poderia usar id_contador direto que o 'move' copiaria igual.
        let meu_id = id_contador;

        println!("Novo cliente conectado: {}", addr);

        let tx = tx.clone();
        let mut rx = tx.subscribe();

        tokio::spawn(async move {
            let (reader, mut writer) = socket.split();
            
            // O BufReader precisa do 'AsyncBufReadExt' importado lá em cima
            // para o método .read_line() funcionar
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            loop {
                tokio::select! {
                    result = reader.read_line(&mut line) => {
                        // Se result for erro ou 0 bytes, encerra a conexão
                        match result {
                            Ok(0) => break,
                            Ok(_) => {
                                let msg_final = format!("{} {} : {}", "Pessoa", meu_id, line);
                                print!("{}", msg_final); // Printa no servidor
                                // Envia para todos
                                let _ = tx.send((msg_final, addr)); 
                                line.clear();
                            }
                            Err(_) => break,
                        }
                    }
                    result = rx.recv() => {
                        if let Ok((msg, sender_addr)) = result {
                            if sender_addr != addr {
                                let _ = writer.write_all(msg.as_bytes()).await;
                            }
                        }
                    }
                }
            }
        });
    }
}