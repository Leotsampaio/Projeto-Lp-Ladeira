mod conexao; 

#[tokio::main]
async fn main() {
    println!("O programa está começando...");
    
    // 2. Chama a função pública que criamos no outro arquivo
    // Como ela é async, precisamos usar .await
    conexao::iniciar().await; 
}