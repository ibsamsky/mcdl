use vergen_gix::{CargoBuilder, Emitter, GixBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gitcl = GixBuilder::default().sha(true).build()?;
    let cargo = CargoBuilder::default().opt_level(true).build()?;

    Emitter::default()
        .add_instructions(&gitcl)?
        .add_instructions(&cargo)?
        .emit()?;
    Ok(())
}
