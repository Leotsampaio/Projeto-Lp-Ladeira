// src/conexao.rs

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

// Adicionamos OpenOptions e File para lidar com arquivos
use tokio::fs::{File, OpenOptions}; 
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};

/// Tipo para mapear endereços para nomes/apelidos
pub type Usuarios = Arc<Mutex<HashMap<SocketAddr, String>>>;
/// Tipo para compartilhar o arquivo de log entre as threads
pub type ArquivoLog = Arc<Mutex<File>>;

/// Função auxiliar para escrever no arquivo de forma segura
async fn salvar_no_historico(arquivo: &ArquivoLog, mensagem: &str) {
    let mut file = arquivo.lock().await;
    // Escreve a mensagem e força a gravação no disco
    if let Err(e) = file.write_all(mensagem.as_bytes()).await {
        eprintln!("Erro ao escrever no log: {}", e);
    }
    // Opcional: flush garante que salvou, mas pode impactar performance se for muito frequente
    let _ = file.flush().await; 
}

pub async fn iniciar() {
    // 1. Preparar o arquivo de Log
    let file = OpenOptions::new()
        .create(true) // Cria se não existir
        .append(true) // Adiciona ao final (não apaga o anterior)
        .open("historico_chat.txt")
        .await
        .expect("Falha ao abrir arquivo de log");

    let arquivo_log: ArquivoLog = Arc::new(Mutex::new(file));

    // Canal global de broadcast
    let (tx, _rx) = broadcast::channel::<(String, SocketAddr)>(100);

    // Estrutura que guarda os usuários conectados
    let usuarios: Usuarios = Arc::new(Mutex::new(HashMap::new()));

    let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();

    println!("--- Servidor de Chat Iniciado ---");
    println!("Rodando em 0.0.0.0:8080");
    println!("Histórico sendo salvo em 'historico_chat.txt'");

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
        let meu_id = id_contador;

        println!("Novo cliente conectado: {}", addr);

        // Clones para a task (incluindo o arquivo de log)
        let tx = tx.clone();
        let mut rx = tx.subscribe();
        let usuarios = usuarios.clone();
        let arquivo_log = arquivo_log.clone(); // Clone do ponteiro para o arquivo

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
                Ok(0) => return,
                Ok(_) => {
                    let n = line.trim().to_string();
                    line.clear();
                    if n.is_empty() { format!("Pessoa{}", meu_id) } else { n }
                }
                Err(_) => return,
            };

            // Salva na tabela de usuários
            {
                let mut users = usuarios.lock().await;
                users.insert(addr, nome.clone());
            }

            // --- LOG: Entrada ---
            let entrada_msg = format!("[Servidor]: {} entrou no chat.\n", nome);
            let _ = tx.send((entrada_msg.clone(), addr));
            salvar_no_historico(&arquivo_log, &entrada_msg).await;

            // Loop principal
            loop {
                tokio::select! {
                    // Lendo mensagem do cliente
                    result = reader.read_line(&mut line) => {
                        match result {
                            Ok(0) => {
                                // Cliente desconectou
                                if let Some(nome_saida) = remove_usuario(&usuarios, addr).await {
                                    // --- LOG: Saída (Desconexão) ---
                                    let saida_msg = format!("[Servidor]: {} saiu do chat.\n", nome_saida);
                                    let _ = tx.send((saida_msg.clone(), addr));
                                    salvar_no_historico(&arquivo_log, &saida_msg).await;
                                }
                                break;
                            }
                            Ok(_) => {
                                let texto_trim = line.trim().to_string();
                                line.clear();

                                if texto_trim.starts_with('/') {
                                    // ===== COMANDOS =====
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
                                                // --- LOG: Saída (Comando) ---
                                                let saida_msg = format!("[Servidor]: {} saiu do chat.\n", nome_saida);
                                                let _ = tx.send((saida_msg.clone(), addr));
                                                salvar_no_historico(&arquivo_log, &saida_msg).await;
                                            }
                                            let _ = writer.write_all(b"Saindo do chat...\n").await;
                                            break;
                                        }
                                        "/help" => {
                                            let ajuda = "Comandos: /list, /nick <nome>, /quit, /help\n";
                                            let _ = writer.write_all(ajuda.as_bytes()).await;
                                        }
                                        "/nick" => {
                                            let novo_nome = argumento.trim();
                                            if !novo_nome.is_empty() {
                                                let mut users = usuarios.lock().await;
                                                if let Some(nome_antigo) = users.insert(addr, novo_nome.to_string()) {
                                                    // --- LOG: Mudança de Nick ---
                                                    let msg = format!("[Servidor]: {} mudou o nome para {}\n", nome_antigo, novo_nome);
                                                    let _ = tx.send((msg.clone(), addr));
                                                    let _ = writer.write_all(format!("Nome alterado para {}\n", novo_nome).as_bytes()).await;
                                                    
                                                    // Drop do lock do users antes de await no log para evitar deadlock
                                                    drop(users); 
                                                    salvar_no_historico(&arquivo_log, &msg).await;
                                                }
                                            }
                                        }
                                        _ => {
                                            let _ = writer.write_all(b"Comando desconhecido.\n").await;
                                        }
                                    }

                                } else {
                                    // ===== MENSAGEM NORMAL =====
                                    let nome_atual = {
                                        let users = usuarios.lock().await;
                                        users.get(&addr).cloned().unwrap_or_else(|| format!("Pessoa{}", meu_id))
                                    };
                                    
                                    // --- LOG: Mensagem ---
                                    let msg_final = format!("[{}]: {}\n", nome_atual, texto_trim);
                                    print!("{}", msg_final); // Print no terminal do servidor
                                    let _ = tx.send((msg_final.clone(), addr));
                                    salvar_no_historico(&arquivo_log, &msg_final).await;
                                }
                            }
                            Err(_) => {
                                if let Some(nome_saida) = remove_usuario(&usuarios, addr).await {
                                    let saida_msg = format!("[Servidor]: {} saiu (erro).\n", nome_saida);
                                    let _ = tx.send((saida_msg.clone(), addr));
                                    salvar_no_historico(&arquivo_log, &saida_msg).await;
                                }
                                break;
                            }
                        }
                    }
                    // Recebendo broadcasts
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

// -----------------------------
// FUNÇÕES PÚBLICAS/HELPERS
// -----------------------------

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