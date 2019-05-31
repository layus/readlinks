with import <nixpkgs> {
  overlays = [ (import ./readlinks-overlay.nix) ];
};

readlinks
