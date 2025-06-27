n = NetAddr("127.0.0.1", 8001);
thisProcess.addOSCRecvFunc({|...m| m.postln;})

x = n.tryConnectTCP({"complete".postln}, {"failure".postln});

n.sendMsg("/hello1", 42.0)

n.tryDisconnectTCP({"disconnected".postln;}, {"discoonnect faild".postln;})


PathName
