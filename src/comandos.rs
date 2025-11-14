use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use tokio::io::AsyncWriteExt;

pub type Tx = UnboundedSender<String>;
pub type Users = Arc<Mutex<HashMap<String, Tx>>>;

/// Verifica se a mensagem é um comando
pub fn is_command(msg: &str) -> bool {
    msg.starts_with('/')
}

/// Função principal que interpreta e executa o comando
pub async fn handle_command(
    user_name: &str,
    cmd: &str,
    users: &Users,
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
) -> bool {
    match cmd {
        "/help" => {
            send_help(writer).await;
            false // continua conectado
        }

        "/list" => {
            send_list(users, writer).await;
            false
        }

        "/quit" => {
            quit_user(user_name, users).await;
            true // indica que é pra desconectar
        }

        _ => {
            let _ = writer.write_all(b"Comando desconhecido. Use /help\n").await;
            false
        }
    }
}

/// Envia lista de comandos
async fn send_help(writer: &mut tokio::net::tcp::WriteHalf<'_>) {
    let msg = 
        "Comandos disponíveis:\n\
        /list - mostra usuários online\n\
        /quit - sair do chat\n\
        /help - mostra esta ajuda\n";

    let _ = writer.write_all(msg.as_bytes()).await;
}

/// Lista usuários conectados
async fn send_list(
    users: &Users,
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
) {
    let users = users.lock().unwrap();

    let mut msg = String::from("Usuários online:\n");
    for (name, _) in users.iter() {
        msg.push_str(&format!("- {}\n", name));
    }

    let _ = writer.write_all(msg.as_bytes()).await;
}

/// Remove usuário e avisa todos
async fn quit_user(user_name: &str, users: &Users) {
    let mut users = users.lock().unwrap();

    // Remove usuário
    users.remove(user_name);

    // Avisa os outros
    let aviso = format!("[Servidor]: {} saiu do chat.\n", user_name);

    for (_, tx) in users.iter() {
        let _ = tx.send(aviso.clone());
    }
}
