DataSteckerIn : UGen {
	*kr {|roomName, host="http://127.0.0.1:8000"|
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
	*kr {|input, roomName, host="http://127.0.0.1:8000"|
		^this.new1('control', input, roomName, host);
	}

	checkInputs {
		^this.checkValidInputs;
	}

	*new1 {|rate, input, roomName, host|
		var roomNameAscii = roomName.ascii;
		var hostAscii = host.ascii;
		var args = [rate, input, roomNameAscii.size, hostAscii.size].addAll(roomNameAscii).addAll(hostAscii);
		^super.new1(*args);
	}
}

SteckerOut : UGen {
	*ar {|input, roomName, host="http://127.0.0.1:8000"|
		^this.new1('audio', input, roomName, host);
	}

	checkInputs {
		^this.checkValidInputs;
	}

	*new1 {|rate, input, roomName, host|
		var roomNameAscii = roomName.ascii;
		var hostAscii = host.ascii;
		var args = [rate, input, roomNameAscii.size, hostAscii.size].addAll(roomNameAscii).addAll(hostAscii);
		^super.new1(*args);
	}
}
