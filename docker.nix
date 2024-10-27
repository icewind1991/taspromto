{ dockerTools
, taspromto
,
}:
dockerTools.buildLayeredImage {
  name = "icewind1991/taspromto";
  tag = "latest";
  maxLayers = 5;
  contents = [
    taspromto
    dockerTools.caCertificates
  ];
  config = {
    Cmd = [ "taspromto" ];
    ExposedPorts = {
      "80/tcp" = { };
    };
  };
}
