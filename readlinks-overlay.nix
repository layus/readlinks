self: super:

with super;

{
  readlinks = rustPlatform.buildRustPackage rec {
    name = "${pname}-${version}";
    version = "0.0.1";
    pname = "readlinks";

    src = lib.cleanSource ./.;
    /*
    src = fetchFromGitHub {
      owner = "layus";
      repo = "readlinks";
      #rev = "${version}";
      rev = "5267ce42cfed0644cae1855181f925a20258fdde";
      sha256 = "13d489z0d474fk7hha824vgyxki8q524v4ibz288f44y33s3pmhp";
    };
    */

    doCheck = false;

    cargoSha256 = "07cs4vqssw7d8zn1yiksq1fxn7zvf7q4g4f5bjzbi8bi8sr9wwrj";
  };
}
