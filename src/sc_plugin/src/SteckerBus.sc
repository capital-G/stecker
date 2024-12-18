
AbstractStecker {

	var <roomName, <hostName;
	var <bus, <synth;
	var <>graceTime = 3, <fresh = true;

	*new { |roomName, input|
		var steckerBus = this.all[roomName];
		if(steckerBus.isNil) {
			steckerBus = super.newCopyArgs(roomName);
			this.all[roomName] = steckerBus
		};
		if(input.notNil) {
			steckerBus.checkRoom;
			steckerBus.startSynth;
			steckerBus.outputUGen(input);
		};
		^steckerBus
	}

	open { |host, maxNumChannels = 1, server| // currently onyl works mono!
		if(hostName.notNil) { Error("There is already a room open.").throw };
		if(maxNumChannels > 1) { Error("Only Mono For Now").throw };
		hostName = host;
		this.initBus(server ? Server.default, maxNumChannels);
	}

	close {
		hostName = nil;
		synth.free;
		bus.free;
	}

	asUGenInput { ^0 }

	// private interface

	startSynth {
		if(synth.isPlaying.not) {
			fork {
				if(fresh.not) { graceTime.wait };
				synth = this.synthFunc.play(bus.server).register;
				fresh = false;
			};
		}
	}

	*newRunning { |roomName|
		var room = this.new(roomName);
		if(room.hostName.isNil) { Error("Room not open.").throw };
		room.startSynth;
		^room
	}

	checkRoom {
		if(roomName.isNil or: hostName.isNil) { Error("Room not open.").throw };
		if(UGen.buildSynthDef.isNil) { Error("Can only add an input inside a synth.").throw };
		if(NodeProxy.buildProxy.notNil) {
			if(NodeProxy.buildProxy.server != bus.server) {
				Error(
					"Each room must be on one server, this one is on '%', tried to add input to room on '%'"
					.format(bus.server, NodeProxy.buildProxy.server)
				).throw
			}
		} {
			"Because a room output can be only on a single server, take care to run this SynthDef only on '%'".format(bus.server).warn
		}
	}


}

Stecker : AbstractStecker {

	classvar <>all;

	*initClass {
		all = IdentityDictionary.new;
	}

	*ar { |roomName, numChannels|
		var room = this.newRunning(roomName);
		if(room.bus.rate != \audio) {
			Error("This is a control rate stecker bus, can't output audio rate").throw
		};
		^room.inputUGen(numChannels)
	}

	initBus { |server, maxNumChannels|
		if(server.serverRunning.not) { Error("Server not running.").throw };
		bus = Bus.audio(server, maxNumChannels);
	}

	synthFunc {
		^{
			var input = InFeedback.ar(bus.index, bus.numChannels);
			SteckerOut.ar(input, roomName, hostName);
			0.0
		}
	}

	// this could become InBus
	inputUGen { |numChannels|
		^InFeedback.ar(bus.index, numChannels.min(bus.numChannels))
	}

	outputUGen { |input|
		^Out.ar(bus.index, input * EnvGate.new)
	}

}


DataStecker : AbstractStecker {

	classvar <>all;

	*initClass {
		all = IdentityDictionary.new;
	}

	*kr { |roomName, numChannels|
		var room = this.newRunning(roomName);
		if(room.bus.rate != \control) {
			Error("This is an audio rate stecker bus, can't output control rate").throw
		};
		^room.inputUGen(numChannels)
	}

	initBus { |server, maxNumChannels|
		if(server.serverRunning.not) { Error("Server not running.").throw };
		bus = Bus.control(server, maxNumChannels);
	}

	synthFunc {
		^{
			var input = In.kr(bus.index, bus.numChannels);
			DataSteckerOut.kr(input, roomName, hostName);
			0.0
		}
	}

	// this could become InBus
	inputUGen { |numChannels|
		^In.kr(bus.index, numChannels.min(bus.numChannels))
	}

	outputUGen { |input|
		^Out.kr(bus.index, input * EnvGate.new)
	}


}




/*


Stecker(\test) === Stecker(\test)

Stecker(\test).open("http://stecker.dennis-scheiba.com", 1);

Stecker(\test).bus
Stecker(\test).startSynth;

(
Ndef(\test1, {
Stecker(\test, SinOsc.ar(500) * 0.1);
});
)

(
Ndef(\test2, {
Stecker(\test, Pulse.ar(120) * 0.1);
0
});
)

Ndef(\test1).free
Ndef(\test2).free

(
Ndef(\out, {
Stecker.ar(\test, 1)
}).play;
)

Stecker(\test).close;

*/
