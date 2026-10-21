{
  lib,
  makeRustPlatform,
  rust-bin,
}:
let
  toolchain = rust-bin.stable.latest.default;
  rustPlatform = makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "lazydn";
  version = (lib.importTOML ../Cargo.toml).package.version;
  src = ../.;
  cargoLock.lockFile = ../Cargo.lock;
  meta = {
    description = "A terminal UI client for dev-null.sh";
    license = lib.licenses.mit;
    mainProgram = "lazydn";
  };
}
