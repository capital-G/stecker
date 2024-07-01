SuperStecker : UGen {
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
