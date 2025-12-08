// src/conexao.rs

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};

/// Tipo para mapear endereços para nomes/apelidos
pub type Usuarios = Arc<Mutex<HashMap<SocketAddr, String>>>;

/// Inicia o servidor de chat (aceita várias conexões e faz broadcast)
pub async fn iniciar() {
    // Canal global de broadcast: (mensagem_formatada, addr_do_remetente)
    let (tx, _rx) = broadcast::channel::<(String, SocketAddr)>(100);

    // Estrutura que guarda os usuários conectados: SocketAddr -> Nome
    let usuarios: Usuarios = Arc::new(Mutex::new(HashMap::new()));

    let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();

    println!("--- Servidor de Chat Iniciado ---");
    println!("Rodando em 0.0.0.0:8080");

    let mut id_contador = 0usize;

    loop {
        let (mut socket, addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("Erro ao aceitar conexão: {}", e);
                continue;
            }
        };

        id_contador += 1;
        let meu_id = id_contador; // id de fallback/log

        println!("Novo cliente conectado: {}", addr);

        // clones para a task
        let tx = tx.clone();
        let mut rx = tx.subscribe();
        let usuarios = usuarios.clone();

        tokio::spawn(async move {
            let (reader, mut writer) = socket.split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            // 1) Perguntar e registrar nome
            if let Err(e) = writer.write_all(b"Digite seu nome/apelido:\n").await {
                eprintln!("Falha ao escrever para {}: {}", addr, e);
                return;
            }

            let nome: String = match reader.read_line(&mut line).await {
                Ok(0) => {
                    eprintln!("{} desconectou sem enviar nome", addr);
                    return;
                }
                Ok(_) => {
                    let n = line.trim().to_string();
                    line.clear();
                    if n.is_empty() {
                        format!("Pessoa{}", meu_id)
                    } else {
                        n
                    }
                }
                Err(e) => {
                    eprintln!("Erro lendo nome de {}: {}", addr, e);
                    return;
                }
            };

            // Salva na tabela de usuários
            {
                let mut users = usuarios.lock().await;
                users.insert(addr, nome.clone());
            }

            // Broadcast de entrada
            let entrada_msg = format!("[Servidor]: {} entrou no chat.\n", nome);
            let _ = tx.send((entrada_msg.clone(), addr));

            // Loop principal
            loop {
                tokio::select! {
                    // Lendo mensagem do cliente
                    result = reader.read_line(&mut line) => {
                        match result {
                            Ok(0) => {
                                // Cliente desconectou
                                if let Some(nome_saida) = remove_usuario(&usuarios, addr).await {
                                    let saida_msg = format!("[Servidor]: {} saiu do chat.\n", nome_saida);
                                    let _ = tx.send((saida_msg.clone(), addr));
                                }
                                break;
                            }
                            Ok(_) => {
                                let texto_trim = line.trim().to_string();
                                line.clear();

                                // ===== COMANDOS =====
                                if texto_trim.starts_with('/') {
                                    let mut partes = texto_trim.splitn(2, ' ');
                                    let comando = partes.next().unwrap_or("");
                                    let argumento = partes.next().unwrap_or("");

                                    match comando {
                                        "/list" => {
                                            let lista = get_lista_usuarios(&usuarios).await;
                                            let _ = writer.write_all(lista.as_bytes()).await;
                                        }

                                        "/quit" => {
                                            if let Some(nome_saida) = remove_usuario(&usuarios, addr).await {
                                                let saida_msg = format!("[Servidor]: {} saiu do chat.\n", nome_saida);
                                                let _ = tx.send((saida_msg.clone(), addr));
                                            }
                                            let _ = writer.write_all(b"Saindo do chat...\n").await;
                                            break;
                                        }

                                        "/help" => {
                                            let ajuda = "Comandos disponíveis:\n/list  - lista usuários online\n/nick <novo_nome> - muda seu apelido\n/quit  - sair do chat\n/help  - mostra esta ajuda\n";
                                            let _ = writer.write_all(ajuda.as_bytes()).await;
                                        }

                                        "/nick" => {
                                            let novo_nome = argumento.trim();
                                            if !novo_nome.is_empty() {
                                                let mut users = usuarios.lock().await;
                                                if let Some(nome_antigo) = users.insert(addr, novo_nome.to_string()) {
                                                    let msg = format!("[Servidor]: {} mudou o nome para {}\n", nome_antigo, novo_nome);
                                                    let _ = tx.send((msg, addr));
                                                    let _ = writer.write_all(format!("Seu nome agora é {}\n", novo_nome).as_bytes()).await;
                                                }
                                            } else {
                                                let _ = writer.write_all(b"Uso correto: /nick <novo_nome>\n").await;
                                            }
                                        }

                                        _ => {
                                            let _ = writer.write_all(b"Comando desconhecido. Use /help para ver os comandos.\n").await;
                                        }
                                    }

                                } else {
                                    // ===== MENSAGEM NORMAL =====
                                    let nome_atual = {
                                        let users = usuarios.lock().await;
                                        users.get(&addr).cloned().unwrap_or_else(|| format!("Pessoa{}", meu_id))
                                    };
                                    let msg_final = format!("[{}]: {}\n", nome_atual, texto_trim);
                                    print!("{}", msg_final);
                                    let _ = tx.send((msg_final, addr));
                                }
                            }
                            Err(e) => {
                                eprintln!("Erro lendo de {}: {}", addr, e);
                                if let Some(nome_saida) = remove_usuario(&usuarios, addr).await {
                                    let saida_msg = format!("[Servidor]: {} saiu do chat.\n", nome_saida);
                                    let _ = tx.send((saida_msg.clone(), addr));
                                }
                                break;
                            }
                        }
                    }

                    // Recebendo broadcasts
                    result = rx.recv() => {
                        match result {
                            Ok((msg, sender_addr)) => {
                                if sender_addr != addr {
                                    if let Err(e) = writer.write_all(msg.as_bytes()).await {
                                        eprintln!("Erro escrevendo para {}: {}", addr, e);
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                eprintln!("Client {} lagged {} mensagens", addr, n);
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                break;
                            }
                        }
                    }
                }
            }
        });
    }
}

// -----------------------------
// FUNÇÕES PÚBLICAS/HELPERS
// -----------------------------

pub async fn registrar_usuario(usuarios: &Usuarios, addr: SocketAddr, nome: String) {
    let mut users = usuarios.lock().await;
    users.insert(addr, nome);
}

pub async fn remove_usuario(usuarios: &Usuarios, addr: SocketAddr) -> Option<String> {
    let mut users = usuarios.lock().await;
    users.remove(&addr)
}

pub async fn get_lista_usuarios(usuarios: &Usuarios) -> String {
    let users = usuarios.lock().await;
    let mut lista = String::from("Usuários online:\n");
    for nome in users.values() {
        lista.push_str(&format!("- {}\n", nome));
    }
    lista
}

pub async fn obter_nomes(usuarios: &Usuarios) -> Vec<String> {
    let users = usuarios.lock().await;
    users.values().cloned().collect()
}
