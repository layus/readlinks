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

    cargoSha256 = "06m8pi99ypjfaih0xpvf2myjdq9rx8rw2q0kybqn5nlmrpnaa3hm";
  };
}
