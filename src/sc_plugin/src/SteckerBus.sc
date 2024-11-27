


SteckerBus {

	var <roomName, <hostName;
	var <bus, <synth;
	var <>graceTime = 3, <fresh = true;

	classvar <>all;

	*initClass {
		all = IdentityDictionary.new;
	}

	*new { |roomName, input|
		var res = all[roomName];
		if(res.isNil) {
			res = super.newCopyArgs(roomName);
			all[roomName] = res
		};
		if(input.notNil) {
			res.checkRoom;
			res.startSynth;
			res.outputUGen(input);
		};
		^res
	}

	*ar { |roomName, numChannels|
		var room = this.newRunning(roomName);
		if(room.bus.rate != \audio) {
			Error("This is a control rate stecker bus, can't output audio rate").throw
		};
		^room.inputUGen(numChannels)
	}

	*kr { |roomName, numChannels|
		var room = this.newRunning(roomName);
		if(room.bus.rate != \control) {
			Error("This is an audio rate stecker bus, can't output control rate").throw
		};
		^room.inputUGen(numChannels)
	}

	open { |host, rate, maxNumChannels = 1, server| // currently onyl works mono!
		if(hostName.notNil) { Error("There is already a room open.").throw };
		if(maxNumChannels > 1) { Error("Only Mono For Now").throw };
		hostName = host;
		this.initBus(server ? Server.default, rate, maxNumChannels);
	}

	close {
		hostName = nil;
		synth.free;
		bus.free;
	}


	// private interface

	initBus { |server, rate, maxNumChannels|
		if(server.serverRunning.not) { Error("Server not running.").throw };
		bus = Bus.perform(rate, server, maxNumChannels);
	}

	synthFunc {
		^if(bus.rate == \audio) {
			{
				var input = InFeedback.ar(bus.index, bus.numChannels);
				SteckerOut.ar(input, roomName, hostName);
				0.0
			}
		} {
			{
				var input = In.kr(bus.index, bus.numChannels);
				DataSteckerOut.kr(input, roomName, hostName);
				0.0
			}
		}
	}

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

	// this could become InBus
	inputUGen { |numChannels|
		^if(bus.rate == \audio) {
			InFeedback.ar(bus.index, numChannels.min(bus.numChannels))
		} {
			In.kr(bus.index, numChannels.min(bus.numChannels))
		}
	}

	outputUGen { |input|
		^if(bus.rate == \audio) {
			Out.ar(bus.index, input * EnvGate.new)
		} {
			Out.kr(bus.index, input * EnvGate.new)
		}
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



/*


SteckerBus(\test) === SteckerBus(\test)

SteckerBus(\test).open("http://stecker.dennis-scheiba.com", \audio, 1);

SteckerBus(\test).bus
SteckerBus(\test).startSynth;

(
Ndef(\test1, {
	SteckerBus(\test, SinOsc.ar(500) * 0.1);
	0
});
)

(
Ndef(\test2, {
	SteckerBus(\test, Pulse.ar(120) * 0.1);
	0
});
)

Ndef(\test1).free
Ndef(\test2).free

(
Ndef(\out, {
	SteckerBus.ar(\test, 1)
}).play;
)

*/