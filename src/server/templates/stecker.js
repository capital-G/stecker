Alpine.store("stecker", {
  HOST: "http://127.0.0.1:8000",
  rooms: [],
  messages: [],
  localSessionDescription: null,
  remoteSessionDescription: null,
  connected: false,
  message: "",
  sendChannel: null,
  createdRoom: false,

  pc: new RTCPeerConnection({
    iceServers: [
      {
        urls: "stun:stun.l.google.com:19302",
      },
    ],
  }),

  async getRooms() {
    let results = await fetch(this.HOST, {
      method: "POST",

      headers: {
        "Content-Type": "application/json",
      },

      body: JSON.stringify({
        query: `{
            rooms {
                name,
                uuid
            }
          }`,
      }),
    });
    let rawRooms = await results.json();
    this.rooms = rawRooms.data.rooms;
  },

  createRtcOffer() {
    this.sendChannel = this.pc.createDataChannel("foo");
    this.sendChannel.onclose = () => this.log("sendChannel has closed");
    this.sendChannel.onopen = () => this.log("sendChannel has opened");
    this.sendChannel.onmessage = (e) => {
      this.log(`Message from DataChannel '${this.sendChannel.label}' payload '${e.data}'`);
    };
    
    this.pc.oniceconnectionstatechange = (e) => this.log(this.pc.iceConnectionState);
    this.pc.onicecandidate = (event) => {
      if (event.candidate === null) {
        this.localSessionDescription = btoa(
          JSON.stringify(this.pc.localDescription)
        );
      }
    };
    
    this.pc.onnegotiationneeded = (e) =>
      this.pc
        .createOffer()
        .then((d) => this.pc.setLocalDescription(d))
        .catch(s.log); 
  },

  async createRoom(name) {
    let results = await fetch(this.HOST, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        query: `
            mutation createRoom($name:String!, $offer:String!) {
              createRoom(name:$name, offer: $offer)
            }
          `,
          variables: {
            name: name,
            offer: s.localSessionDescription
          }
      }),
    });
    let rawResponse = await results.json();
    console.log(rawResponse);
    this.remoteSessionDescription = rawResponse.data.createRoom;

    this.pc.setRemoteDescription(new RTCSessionDescription(JSON.parse(atob(this.remoteSessionDescription))));

    this.createdRoom = true;
    this.getRooms();
  },

  log(message) {
    this.messages.push(message);
  },

  sendMessage() {
    console.log(`Send message ${this.message}`);
    this.sendChannel.send(this.message);
    this.log(`Send message: ${this.message}`);
    this.message = "";
  },


  async joinRoom(name) {
    let results = await fetch(this.HOST, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        query: `
          mutation joinRoom($name: String!, $offer:String!) {
            joinRoom(name:$name, offer: $offer)
          }
          `,
          variables: {
            name,
            offer: s.localSessionDescription
          }
      }),
    });
    let rawResponse = await results.json();
    this.remoteSessionDescription = rawResponse.data.joinRoom;

    this.pc.setRemoteDescription(new RTCSessionDescription(JSON.parse(atob(this.remoteSessionDescription))));
  },

  setup() {
    this.getRooms();
    this.createRtcOffer();
  }
});

s = Alpine.store("stecker");
s.setup();
