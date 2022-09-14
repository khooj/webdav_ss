{ stdenv, fetchurl }:
let version = "0.13"; in stdenv.mkDerivation {
  inherit version;
  pname = "litmus";
  src = fetchurl {
    url = "http://webdav.org/neon/litmus/litmus-${version}.tar.gz";
    sha256 = "sha256-CdYVlYEhcGRE22fgnEDfX3U8zx+hSEb960OSmKqaw/8=";
  };
}
