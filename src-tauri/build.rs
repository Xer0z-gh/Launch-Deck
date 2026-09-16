fn main() {
    // Tell cargo the frontend bundle is an input to this crate.
    //
    // `tauri_build::build()` embeds everything under `frontendDist` into the
    // binary at compile time, but nothing tells cargo that those files are
    // build inputs. So a frontend-only change rebuilds `dist/` and then does
    // not relink -- `tauri build` reports success and ships a binary carrying
    // the PREVIOUS frontend.
    //
    // That is a silent wrong-answer bug, and it looks exactly like a fix that
    // did not work: the source is right, the build is green, the app still
    // misbehaves. It cost a full verification cycle here -- a guard reported
    // thirty rows still carrying an inline style the source no longer sets,
    // and the exe turned out to be fifteen minutes older than the bundle it
    // was supposed to contain.
    //
    // Cargo walks a directory given here recursively, so this covers every
    // asset. `index.html` is listed separately because its name is stable
    // while the hashed filenames underneath it are not.
    println!("cargo:rerun-if-changed=../dist");
    println!("cargo:rerun-if-changed=../dist/index.html");

    tauri_build::build();
}
