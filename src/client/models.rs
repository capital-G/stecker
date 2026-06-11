use clap::ValueEnum;

#[derive(ValueEnum, Clone)]
pub enum ClientRoomType {
    Chat,
    Float,
}
