//! Offline acceptance assets are supplied explicitly; this loader cannot download a model.
use super::*;

pub(crate) fn cached_multilingual(directory: &std::path::Path) -> MemoryEmbeddingRuntime {
    use fastembed::{InitOptionsUserDefined, Pooling, TokenizerFiles, UserDefinedEmbeddingModel};
    let read = |name: &str| std::fs::read(directory.join(name)).expect("explicit cached E5 asset");
    let model = UserDefinedEmbeddingModel::new(
        read("onnx/model.onnx"),
        TokenizerFiles {
            tokenizer_file: read("tokenizer.json"),
            config_file: read("config.json"),
            special_tokens_map_file: read("special_tokens_map.json"),
            tokenizer_config_file: read("tokenizer_config.json"),
        },
    )
    .with_pooling(Pooling::Mean);
    let model = TextEmbedding::try_new_from_user_defined(model, InitOptionsUserDefined::new())
        .expect("offline cached multilingual E5 model");
    MemoryEmbeddingRuntime::from_provider(MemoryEmbeddingProvider::FastEmbed(
        FastEmbedMemoryEmbedder {
            project_session: uuid::Uuid::new_v4().to_string(),
            model_name: DEFAULT_FASTEMBED_MODEL.into(),
            dimension: 384,
            prompt_style: FastEmbedPromptStyle::E5,
            model: Arc::new(Mutex::new(model)),
        },
    ))
}
