Stecker {
	classvar <>host;
	classvar seed;

	*initClass {
		host = "https://stecker.dennis-scheiba.com";
		seed = 10e6.rand.asInteger;
	}

	// hasher is surjective within one sclang boot
	*hasher {|string|
		// convert to string in order to iterate over the numbers...
		// alt: use mod10 with while loop...
		var hashedString = (string.asString.hash.abs + seed).asString;
		// add ascii offset of 50 to get normal/printable chars
		hashedString = hashedString.collect({|c| (c.asInteger+50).asAscii});
		^hashedString;
	}
}

SteckerOSC {
	classvar <>host;
	classvar <>port;

	classvar <>onRoomCreated;
	classvar <>onRoomUpdated;
	classvar <>onRoomDeleted;
	classvar <>onRoomMessage;

	classvar <>onDispatcherCreated;
	classvar <>onDispatcherDeleted;

	classvar <lastPing;

	classvar <netAddr;
	// we can not add methods to addOSCRecvFunc, so we need to
	// store a variable which holds a function
	classvar recvFunc;

	classvar newsCallbacks;

	*initClass {
		host = "osc.stecker.dennis-scheiba.com";
		port = 1337;
		newsCallbacks = ();

		recvFunc = {|msg, time, replyAddr, recvPort|
			if(replyAddr == netAddr, {
				switch(msg[0])
				{"/ping".asSymbol} {lastPing = time;}
				{"/error".asSymbol} {msg[1].asString.warn;}
				{"/reply".asSymbol} {"Stecker: %".format(msg[1]).postln;}

				{"/createdRoom".asSymbol} {onRoomCreated.value(*msg[1..])}
				{"/updatedRoom".asSymbol} {onRoomUpdated.value(*msg[1..])}
				{"/deletedRoom".asSymbol} {onRoomDeleted.value(*msg[1..])}
				{"/room".asSymbol} {
					// roomName event eventArgs
					if(newsCallbacks[msg[1]].notNil, {
						newsCallbacks[msg[1]].value(*msg[2..]);
					});
					onRoomMessage.value(*msg[1..]);
				}

				{"/createdDispatcher".asSymbol} {onDispatcherCreated.value(*msg[1..])}
				{"/deletedDispatcher".asSymbol} {onDispatcherDeleted.value(*msg[1..])}
			});
		}
	}

	*connect {|callback|
		if(SteckerOSC.connected.not, {
			netAddr = NetAddr(host, port);
			netAddr.tryConnectTCP(
				onComplete: {
					"Connected".postln;
					thisProcess.addOSCRecvFunc(recvFunc);
					callback.value();
				},
				onFailure: {
					"Failed to connect to Stecker OSC host".warn;
				},
			);
		}, {
			"Already connected".warn;
		});
	}

	*disconnect {
		if(SteckerOSC.connected, {
			thisProcess.removeOSCRecvFunc(recvFunc);
			netAddr.disconnect;
		});
	}

	*connected {
		^if(netAddr.notNil, {
			netAddr.isConnected;
		}, {
			false;
		});
	}

	*createDispatcher {|name, password, rule, timeout=1000, dispatcherType=nil, returnRoomPrefix=nil|
		if(SteckerOSC.connected.not, {
			"SteckerOSC is not connected".warn;
			^this;
		});
		netAddr.sendMsg(
			"/createDispatcher",
			name,
			password,
			rule,
			timeout.asInteger,
			returnRoomPrefix,
			dispatcherType ? SteckerOSC.dispatcherNextRandom,
		);
	}

	*onRoomNews {|roomName, callback|
		newsCallbacks[roomName.asSymbol] = callback;
	}

	*dispatcherRandom {
		^"random";
	}

	*dispatcherNextAlpha {
		^"nextfreealpha";
	}

	*dispatcherNextRandom {
		^"nextfreerandom";
	}
}

DataSteckerIn : UGen {
	*kr {|roomName, host=nil|
		host = host ? Stecker.host;
		^this.new1('control', roomName, host);
	}

	checkInputs {
		^this.checkValidInputs;
	}

	*new1 {|rate, roomName, host|
		var roomNameAscii = roomName.ascii;
		var hostAscii = host.ascii;
		var args = [rate, roomNameAscii.size, hostAscii.size].addAll(roomNameAscii).addAll(hostAscii);
		^super.new1(*args);
	}
}

DataSteckerOut : UGen {
	*kr {|input, roomName, password=nil, host=nil|
		host = host ? Stecker.host;
		password = password ?? {Stecker.hasher(roomName)};

		if(input.asArray.size > 1, {
			"Stecker only supports mono channels - reducing input signal to first channel".warn;
			input = input[0];
		});

		^this.new1('control', input, roomName, password, host);
	}

	checkInputs {
		^this.checkValidInputs;
	}

	*new1 {|rate, input, roomName, password, host|
		var roomNameAscii = roomName.ascii;
		var passwordAscii = password.ascii;
		var hostAscii = host.ascii;
		var args = [rate, input, roomNameAscii.size, passwordAscii.size,  hostAscii.size].addAll(roomNameAscii).addAll(passwordAscii).addAll(hostAscii);
		^super.new1(*args);
	}
}

SteckerIn : UGen {
	*ar {|roomName, host=nil|
		host = host ? Stecker.host;
		^this.new1('audio', roomName, host);
	}

	checkInputs {
		^this.checkValidInputs;
	}

	*new1 {|rate, roomName, host|
		var roomNameAscii = roomName.ascii;
		var hostAscii = host.ascii;
		var args = [rate, roomNameAscii.size, hostAscii.size].addAll(roomNameAscii).addAll(hostAscii);
		^super.new1(*args);
	}
}

SteckerOut : UGen {
	*ar {|input, roomName, password=nil, host=nil|
		host = host ? Stecker.host;
		password = password ?? {Stecker.hasher(roomName)};

		if(input.asArray.size > 1, {
			"Stecker only supports mono channels - reducing input signal to first channel".warn;
			input = input[0];
		});

		^this.new1('audio', input, roomName, password, host);
	}

	checkInputs {
		^this.checkValidInputs;
	}

	*new1 {|rate, input, roomName, password, host|
		var roomNameAscii = roomName.ascii;
		var passwordAscii = password.ascii;
		var hostAscii = host.ascii;
		var args = [rate, input, roomNameAscii.size,  passwordAscii.size, hostAscii.size].addAll(roomNameAscii).addAll(passwordAscii).addAll(hostAscii);
		^super.new1(*args);
	}
}
