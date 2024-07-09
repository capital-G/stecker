SuperSteckerIn : UGen {
	*kr {|roomName|
		^this.new1('control', roomName);
	}

	checkInputs {
		^this.checkValidInputs;
	}

	*new1 {|rate,roomName|
		var ascii = roomName.ascii;
		^super.new1(*[rate, ascii.size].addAll(ascii));
	}
}

SuperSteckerOut : UGen {
	*kr {|input, roomName|
		^this.new1('control', input, roomName);
	}

	checkInputs {
		^this.checkValidInputs;
	}

	*new1 {|rate, input, roomName|
		var ascii = roomName.ascii;
		^super.new1(*[rate, input, ascii.size].addAll(ascii));
	}
}
