use ai_runtime::{Backend, Config, Message};
fn main() -> Result<(), String> {
    let started = std::time::Instant::now();
    let backend = Backend::start(&Config::default())?;
    println!(
        "Local model ready in {:.2}s; PID {:?}",
        started.elapsed().as_secs_f64(),
        backend.process_id()
    );
    for prompt in [
        "Responda apenas com o nome da capital da França.",
        "Quanto é 12 mais 7? Responda brevemente.",
        "Sugira em uma frase como organizar as abas de um projeto de trabalho.",
    ] {
        let started = std::time::Instant::now();
        let response = backend.complete(&[Message::user(prompt)], None)?;
        println!(
            "Prompt: {prompt}\nOutput: {}\nTokens: {:?}; {:.2}s\n",
            response.content,
            response.completion_tokens,
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
