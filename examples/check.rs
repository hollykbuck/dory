fn main() {
    println!("config_dir: {:?}", dirs::config_dir());
    println!("home_dir: {:?}", dirs::home_dir());
    println!("preference_dir: {:?}", dirs::preference_dir());
    println!("executable: {:?}", std::env::current_exe().ok());
}
