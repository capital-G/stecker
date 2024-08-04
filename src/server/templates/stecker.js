Alpine.store("stecker", {
    HOST: `${window.location.protocol}//${window.location.host}`,
    rooms: [],
    messages: [],
    localSessionDescription: null,
    remoteSessionDescription: null,
    connected: false,
    message: "",

    floatChannel: null,
    metaChannel: null,
    chatChannel: null,
    audioChannel: null,

    createdRoom: false,
    floatValue: 0.0,
    chatValue: "",

    roomType: null,

    pc: new RTCPeerConnection({
        iceServers: [
            {
                urls: "stun:stun.l.google.com:19302",
            },
        ],
    }),

    async getRooms(roomType) {
        let results = await fetch(this.HOST, {
            method: "POST",

            headers: {
                "Content-Type": "application/json",
            },

            body: JSON.stringify({
                query: `{
                    rooms(roomType: ${roomType}) {
                        uuid,
                        name,
                        numListeners,
                        roomType,
                    }
                }`,
            }),
        });
        if (!results.ok) {
            alert(`Error fetching room: ${results.text()}`);
            return;
        }
        let rawRooms = await results.json();
        this.rooms = rawRooms.data.rooms;
    },

    createFloatChannel() {
        this.floatChannel = this.pc.createDataChannel("float");
        this.floatChannel.onclose = () => this.log(`floatChannel has closed`);
        this.floatChannel.onopen = () => this.log(`floatChannel has opened`);
        this.floatChannel.onmessage = async (e) => {
            let dataView = new DataView(await e.data.arrayBuffer());
            this.floatValue = dataView.getFloat32();
        };
    },

    async createChatChannel() {
        this.chatChannel = this.pc.createDataChannel("chat");
        this.chatChannel.onclose = () => this.log(`chatChannel has closed`);
        this.chatChannel.onopen = () => this.log(`chatChannel has opened`);
        this.chatChannel.onmessage = async (e) => {
            let text = await e.data.text();
            this.log(`CHAT: ${text}`);
        };
    },

    createMetaChannel() {
        this.metaChannel = this.pc.createDataChannel("meta");
        this.metaChannel.onclose = () => this.log(`metaChannel has closed`);
        this.metaChannel.onopen = () => this.log(`metaChannel has opened`);
        this.metaChannel.onmessage = async (e) => {
            let text = await e.data.text();
            this.log(`META: ${text}`);
        };
    },

    async createAudioChannel() {
        let that = this;
        return new Promise((resolve, reject) => {
            console.log("Try to access a media device");
            navigator.mediaDevices.getUserMedia({ video: false, audio: true })
                .then(stream => {
                    stream.getTracks().forEach(track => {
                        that.pc.addTrack(track, stream);
                    });
                    document.getElementById('audio-playback').srcObject = stream;
                    resolve();
                })
                .catch(error => {
                    console.error(`Error obtaining media device: ${error}`);
                    reject(error);
                });
        });
    },

    async listenForAudioChannel() {
        this.pc.addTransceiver('audio')

        this.pc.ontrack = function (event) {
          var el = document.getElementById('audio-playback')
          el.srcObject = event.streams[0]
          el.autoplay = true
          el.controls = true
        }
    },

    async generateSessionDescription() {
        let that = this;
        return new Promise((resolve) => {
            that.pc.oniceconnectionstatechange = (e) => that.log(that.pc.iceConnectionState);

            that.pc.oniceconnectionstatechange = (e) => that.log(that.pc.iceConnectionState);
            that.pc.onicecandidate = (event) => {
              if (event.candidate === null) {
                that.localSessionDescription = btoa(
                  JSON.stringify(that.pc.localDescription)
                );
                resolve();
              }
            };

            that.pc
            .createOffer()
            .then((d) => {
                that.pc.setLocalDescription(d);
                // resolve();
            })
            .catch(s.log);
        });
    },

    async createRoom(name, roomType) {
        this.roomType = roomType;

        this.createMetaChannel();
        if (roomType == "FLOAT") {
            this.createFloatChannel();
        } else if (roomType == "CHAT") {
            this.createChatChannel();
        } else if (roomType == "AUDIO") {
            await this.createAudioChannel();
        }
        await this.generateSessionDescription();

        let response = await fetch(this.HOST, {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
            },
            body: JSON.stringify({
                query: `
            mutation createRoom($name:String!, $offer:String!, $roomType: RoomType!) {
              createRoom(name:$name, offer: $offer, roomType: $roomType)
            }
          `,
                variables: {
                    name: name,
                    offer: s.localSessionDescription,
                    roomType,
                },
            }),
        });
        if (!response.ok) {
            alert(`Error during room creation: ${response.text()}`);
            return;
        }
        let jsonResponse = await response.json();
        console.log(`JSON response from API: ${jsonResponse}`);
        if (jsonResponse.errors !== undefined) {
            alert(`Error during room creation: ${JSON.stringify(jsonResponse.errors)}`);
            return;
        }
        this.remoteSessionDescription = jsonResponse.data.createRoom;

        this.pc.setRemoteDescription(
            new RTCSessionDescription(JSON.parse(atob(this.remoteSessionDescription)))
        );
        this.connected = true;

        this.createdRoom = true;
    },

    log(message) {
        this.messages.push(message);
    },

    sendFloatValue() {
        console.log(`Send float value ${this.floatValue}`);

        // Create an ArrayBuffer with a size in bytes
        const buffer = new ArrayBuffer(4);
        const view = new DataView(buffer);
        view.setFloat32(0, parseFloat(this.floatValue), false);

        this.floatChannel.send(buffer);
    },

    sendChatValue() {
        console.log(`Send chat value ${this.chatValue}`);
        let blob = new Blob([this.chatValue], { type: 'text/plain' });
        this.chatChannel.send(blob);
        this.chatValue = "";
    },

    async joinRoom(name, roomType) {
        this.roomType = roomType;

        this.createMetaChannel();
        if (roomType == "FLOAT") {
            this.createFloatChannel();
        } else if (roomType == "CHAT") {
            this.createChatChannel();
        } else if (roomType == "AUDIO") {
            await this.listenForAudioChannel();
        }

        await this.generateSessionDescription();

        let results = await fetch(this.HOST, {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
            },
            body: JSON.stringify({
                query: `
                mutation joinRoom($name: String!, $offer:String!, $roomType: RoomType!) {
                    joinRoom(name:$name, offer: $offer, roomType: $roomType)
                }
                `,
                variables: {
                    name,
                    offer: s.localSessionDescription,
                    roomType,
                },
            }),
        });
        let rawResponse = await results.json();
        this.remoteSessionDescription = rawResponse.data.joinRoom;

        this.pc.setRemoteDescription(
            new RTCSessionDescription(JSON.parse(atob(this.remoteSessionDescription)))
        );
        this.connected = true;
    },

    setup() {
        // this.generateSessionDescription();
    },
});

s = Alpine.store("stecker");
s.setup();
