SuperStecker : UGen {
	*kr {
		^this.multiNew('control');
	}
	checkInputs {
		/* TODO */
		^this.checkValidInputs;
	}
}
