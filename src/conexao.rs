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

            // 1) Perguntar e registrar nome (João)
            if let Err(e) = writer.write_all(b"Digite seu nome/apelido:\n").await {
                eprintln!("Falha ao escrever para {}: {}", addr, e);
                return;
            }

            let nome: String = match reader.read_line(&mut line).await {
                Ok(0) => {
                    // cliente desconectou antes de enviar nome
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

            // Broadcast de entrada: "[Servidor]: Nome entrou no chat."
            let entrada_msg = format!("[Servidor]: {} entrou no chat.\n", nome);
            // Pode falhar se não houver assinantes, ignore o erro
            let _ = tx.send((entrada_msg.clone(), addr));

            // 2) Loop principal: ler mensagens do cliente e broadcast (formatadas)
            loop {
                tokio::select! {
                    result = reader.read_line(&mut line) => {
                        match result {
                            Ok(0) => {
                                // EOF: cliente desconectou
                                if let Some(nome_saida) = remove_usuario(&usuarios, addr).await {
                                    let saida_msg = format!("[Servidor]: {} saiu do chat.\n", nome_saida);
                                    let _ = tx.send((saida_msg.clone(), addr));
                                }
                                break;
                            }
                            Ok(_) => {
                                let texto = line.clone(); // inclui \n
                                line.clear();

                                let texto_trim = texto.trim_end().to_string();

                                // Tratamento simples de comandos aqui (funciona para o grupo).
                                // Matheus pode optar por substituir essa lógica pelo interpretador completo.
                                if texto_trim == "/list" {
                                    // responde só para este cliente
                                    let lista = get_lista_usuarios(&usuarios).await;
                                    if let Err(e) = writer.write_all(lista.as_bytes()).await {
                                        eprintln!("Erro escrevendo /list para {}: {}", addr, e);
                                        // continua a execução; não forçamos desconexão
                                    }
                                    continue; // não broadcast /list
                                }

                                if texto_trim == "/quit" {
                                    // Remove usuário e avisa todos
                                    if let Some(nome_saida) = remove_usuario(&usuarios, addr).await {
                                        let saida_msg = format!("[Servidor]: {} saiu do chat.\n", nome_saida);
                                        let _ = tx.send((saida_msg.clone(), addr));
                                    }
                                    // Fecha a conexão para este cliente
                                    break;
                                }

                                // Mensagem normal -> formata e broadcast
                                let nome_atual = {
                                    let users = usuarios.lock().await;
                                    users.get(&addr).cloned().unwrap_or_else(|| format!("Pessoa{}", meu_id))
                                };

                                let msg_final = format!("[{}]: {}\n", nome_atual.trim(), texto_trim);
                                print!("{}", msg_final); // log no servidor

                                // Envia para todos (quem receber faz filtro para não ecoar para o remetente)
                                let _ = tx.send((msg_final.clone(), addr));
                            }
                            Err(e) => {
                                eprintln!("Erro lendo de {}: {}", addr, e);
                                // tratar como desconexão
                                if let Some(nome_saida) = remove_usuario(&usuarios, addr).await {
                                    let saida_msg = format!("[Servidor]: {} saiu do chat.\n", nome_saida);
                                    let _ = tx.send((saida_msg.clone(), addr));
                                }
                                break;
                            }
                        }
                    }

                    // Recebendo broadcasts de outros clientes
                    result = rx.recv() => {
                        match result {
                            Ok((msg, sender_addr)) => {
                                // Não reenvia para o próprio remetente
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
                                // Canal fechado; finaliza
                                break;
                            }
                        }
                    }
                } // tokio::select
            } // loop -> final da conexão
        }); // tokio::spawn
    } // loop accept
} // fn iniciar

// -----------------------------
// FUNÇÕES PÚBLICAS/HELPERS PARA INTEGRAÇÃO
// -----------------------------

/// Registra um usuário manualmente (útil em testes ou integração)
pub async fn registrar_usuario(usuarios: &Usuarios, addr: SocketAddr, nome: String) {
    let mut users = usuarios.lock().await;
    users.insert(addr, nome);
}

/// Remove o usuário do mapa e retorna o nome (se havia)
pub async fn remove_usuario(usuarios: &Usuarios, addr: SocketAddr) -> Option<String> {
    let mut users = usuarios.lock().await;
    users.remove(&addr)
}

/// Retorna a lista formatada de usuários (string pronta para envio ao cliente)
/// Uso: let lista = get_lista_usuarios(&usuarios).await;
pub async fn get_lista_usuarios(usuarios: &Usuarios) -> String {
    let users = usuarios.lock().await;
    let mut lista = String::from("Usuários online:\n");
    for nome in users.values() {
        lista.push_str(&format!("- {}\n", nome));
    }
    lista
}

/// Recupera uma cópia do vetor de nomes (útil para testes ou outras integrações)
pub async fn obter_nomes(usuarios: &Usuarios) -> Vec<String> {
    let users = usuarios.lock().await;
    users.values().cloned().collect()
}
