//! Intègre l'icône 3D (`assets/icon/forge-bar.ico`) dans l'exécutable : barre des tâches, menu Démarrer,
//! Explorateur et « Applications installées » l'affichent depuis cette ressource.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/forge-bar.rc");
    println!("cargo:rerun-if-changed=../../assets/icon/forge-bar.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("../../assets/icon/forge-bar.rc", embed_resource::NONE)
            .manifest_required()
            .expect("compilation de la ressource icône");
    }
}
