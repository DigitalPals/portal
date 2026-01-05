use iced::Font;

/// JetBrains Mono Nerd Font - embedded for terminal rendering
pub const JETBRAINS_MONO_NERD: Font = Font::with_name("JetBrainsMono Nerd Font");

/// Raw font bytes for loading at startup
pub const JETBRAINS_MONO_NERD_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");
