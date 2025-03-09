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
