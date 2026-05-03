use vergen::Emitter;
use vergen_gitcl::GitclBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut emitter = Emitter::default();
    
    emitter.add_instructions(&vergen::BuildBuilder::all_build()?)?;
    emitter.add_instructions(&GitclBuilder::all_git()?)?;
    
    emitter.emit()?;

    Ok(())
}
