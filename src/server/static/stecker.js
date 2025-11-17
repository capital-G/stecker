class SteckerConnection {
    constructor() {
        this.peerConnection = new RTCPeerConnection({
            iceServers: [
                {
                    urls: "stun:stun.l.google.com:19302",
                },
            ],
        });

        /**
         * @type {string|null}
         */
        this.localDescription = null;
    }

    /**
     * @param {string|null} attachToHTMLPlayer - Optional parameter to specify HTML player to which
     * @returns {Promise<MediaStream>}
     */
    async createAudioChannel(attachToHTMLPlayer=null) {
        return new Promise((resolve, reject) => {
            console.log("Try to access a media device");
            navigator.mediaDevices.getUserMedia({ video: false, audio: true })
                .then(stream => {
                    stream.getTracks().forEach(track => {
                        this.peerConnection.addTrack(track, stream);
                    });
                    if(attachToHTMLPlayer !== null) {
                        document.getElementById(attachToHTMLPlayer).srcObject = stream;
                    }
                    resolve(stream);
                    Alpine.store("stecker").isPlaying = true;
                })
                .catch(error => {
                    console.error(`Error obtaining media device: ${error}`);
                    reject(error);
                });
        });
    }

    /**
     * @param {HTMLAudioElement} htmlPlayer - ID of HTML element to attach to
     */
    async listenForAudioChannel(htmlPlayer) {
        this.peerConnection.addTransceiver('audio')

        this.peerConnection.ontrack = function (event) {
        //   var el = document.getElementById(attachToHTMLPlayer);
          Alpine.store("stecker").isPlaying = true;
          htmlPlayer.srcObject = event.streams[0]
          htmlPlayer.autoplay = true
          htmlPlayer.controls = true
        }
    }

    /**
     * @returns {Promise<string>}
     */
    async generateLocalSessionDescription() {
        let that = this;
        Alpine.store("stecker").isConnecting = true;
        return new Promise((resolve) => {
            that.peerConnection.oniceconnectionstatechange = (e) => console.log(`ICE connection state: ${that.peerConnection.iceConnectionState}`);
            that.peerConnection.onicecandidate = (event) => {
              if (event.candidate === null) {
                let localSessionDescription = btoa(
                  JSON.stringify(that.peerConnection.localDescription)
                );
                that.localSessionDescription = localSessionDescription;
                resolve(localSessionDescription);
              }
            };

            that.peerConnection
            .createOffer()
            .then((d) => {
                that.peerConnection.setLocalDescription(d);
                // resolve();
            })
            .catch((e) => console.log(`Some problems obtaining an offer: ${e}`));
        });
    }
}

class SteckerDataChannel {
    /**
     * @param {SteckerConnection} steckerConnection
     * @param {string} roomType - one of "float", "chat" or "meta"
     * @param {function(string|number): void} messageCallback - will be called if a message is received
     */
    constructor(steckerConnection, roomType, messageCallback) {
        this.steckerConnection = steckerConnection;
        this.messageCallback = messageCallback;
        this.roomType = roomType;

        this.channel = this.steckerConnection.peerConnection.createDataChannel(this.roomType);
        this.channel.onclose = () => console.log(`${this.roomType}Channel has closed`);
        this.channel.onopen = () => console.log(`${this.roomType}Channel has opened`);
        this.channel.onmessage = async (e) => {
            switch (this.roomType) {
                case "float":
                    let dataView = new DataView(await e.data.arrayBuffer());
                    let floatValue = dataView.getFloat32();
                    this.messageCallback(floatValue);
                    break;
                default:
                    let text = await e.data.text();
                    this.messageCallback(text);
            }
        };
    }

    /**
     * @param {string|number} value
     */
    sendValue(value) {
        switch (this.roomType) {
            case "float":
                console.log(`Send float ${value}`);
                // Create an ArrayBuffer with a size in bytes
                const buffer = new ArrayBuffer(4);
                const view = new DataView(buffer);
                view.setFloat32(0, parseFloat(value), false);
                this.channel.send(buffer);
                break;
            default:
                console.log(`Send text "${value}"`);
                let blob = new Blob([value], { type: 'text/plain' });
                this.channel.send(blob);
        }
    }
}

Alpine.store("stecker", {
    HOST: `${window.location.protocol}//${window.location.host}/graphql`,
    rooms: [],
    messages: [],
    /**
     * @type {null | SteckerDataChannel}
     */
    steckerDataChannel: null,
    steckerAudioChannelIn: null,
    steckerAudioChannelOut: null,

    allowSendFloat: false,
    allowSendChat: false,

    // stores if we created a room
    createdRoom: false,
    // stores if we are connected to a room
    connectedRoom: false,

    floatValue: 0.0,
    chatValue: "",

    isConnecting: false,
    isPlaying: false,

    /**
     *
     * @param {string} roomType
     * @returns {void}
     */
    async getRooms(roomType) {
        let results = await fetch(this.HOST, {
            method: "POST",

            headers: {
                "Content-Type": "application/json",
            },

            body: JSON.stringify({
                query: `
                    query getRooms($roomType: RoomType!) {
                        rooms(roomType: $roomType) {
                            uuid,
                            name,
                            numListeners,
                            roomType,
                        }
                    }
                `,
                variables: {
                    roomType: roomType.toUpperCase(),
                }
            }),
        });
        if (!results.ok) {
            alert(`Error fetching room: ${results.text()}`);
            return;
        }
        let rawRooms = await results.json();
        this.rooms = rawRooms.data.rooms;
    },

    /**
     *
     * @param {string} name
     * @param {string} roomType
     * @returns {void}
     */
    async createRoom(name, roomType) {
        let steckerConnection = new SteckerConnection();

        // we actually don't need to attach this to our alpine store
        new SteckerDataChannel(steckerConnection, "meta", (msg) => {
            this.log(`META(${name}): ${msg}`);
        });

        switch (roomType) {
            case "float":
                this.steckerDataChannel = new SteckerDataChannel(steckerConnection, "float", (msg) => {
                    // @todo is this actually "this"?
                    this.floatValue = msg;
                });
                this.allowSendFloat = true;
                break;
            case "chat":
                this.steckerDataChannel = new SteckerDataChannel(steckerConnection, "chat", (msg) => {
                    this.log(`CHAT: ${msg}`);
                });
                this.allowSendChat = true;
                break;
            case "audio":
                await steckerConnection.createAudioChannel();
                break;
            default:
                alert(`Unknown room type "${roomType}"`);
                return;
        }

        let localSessionDescription = await steckerConnection.generateLocalSessionDescription();

        let response = await fetch(this.HOST, {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
            },
            body: JSON.stringify({
                query: `
                    mutation createRoom($name:String!, $offer:String!, $roomType: RoomType!) {
                        createRoom(name:$name, offer: $offer, roomType: $roomType) {
                            offer,
                            password,
                        }
                    }
                `,
                variables: {
                    name: name,
                    offer: localSessionDescription,
                    roomType: roomType.toUpperCase(),
                },
            }),
        });
        if (!response.ok) {
            alert(`Error during room creation: ${response.text()}`);
            return;
        }
        let jsonResponse = await response.json();
        console.log(`JSON response from API: `, jsonResponse);
        if (jsonResponse.errors !== undefined) {
            alert(`Error during room creation: ${JSON.stringify(jsonResponse.errors)}`);
            return;
        }

        // put this into stecker connection class?
        let remoteSessionDescription = jsonResponse.data.createRoom.offer;
        steckerConnection.peerConnection.setRemoteDescription(
            new RTCSessionDescription(JSON.parse(atob(remoteSessionDescription)))
        );
        this.createdRoom = true;
    },

    log(message) {
        this.messages.push(message);
    },

    sendFloatValue() {
        this.steckerDataChannel.sendValue(this.floatValue);
    },

    sendChatValue() {
        this.steckerDataChannel.sendValue(this.chatValue);
        this.chatValue = "";
    },

    /**
     *
     * @param {String} dispatcherName
     */
    async accessDispatcher(dispatcherName) {
        let response = await fetch(this.HOST, {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
            },
            body: JSON.stringify({
                query: `
                mutation accessDispatcher($dispatcherName: String!) {
                    accessDispatcher(name: $dispatcherName) {
                        name,
                        roomType,
                    }
                }
                `,
                variables: {
                    dispatcherName
                },
            }),
        });
        return await response.json();
    },

    /**
     *
     * @param {string} name
     * @param {string} roomType
     * @param {string|null} returnRoomPrefix
     */
    async joinRoom(name, roomType, returnRoomPrefix) {
        // @todo derived from graphQL, but in js we use lowercase
        roomType = roomType.toLowerCase();

        if(returnRoomPrefix != null) {
            const returnRoomName = `${returnRoomPrefix}${name}`;
            console.log(`Create return room ${returnRoomName}`);
            this.createRoom(returnRoomName, roomType);
        }

        let steckerConnection = new SteckerConnection();

        new SteckerDataChannel(steckerConnection, "meta", (msg) => {
            this.log(`META(${name}): ${msg}`);
        });

        switch (roomType) {
            case "float":
                this.steckerDataChannel = new SteckerDataChannel(steckerConnection, "float", (msg) => {
                    this.floatValue = msg;
                });
                break;
            case "chat":
                this.steckerDataChannel = new SteckerDataChannel(steckerConnection, "chat", (msg) => {
                    this.log(`Chat(${name}): ${msg}`);
                });
                break;
            case "audio":
                if (this.steckerAudioChannelIn !== null) {
                    this.steckerAudioChannelIn.close();
                };
                let htmlPlayer = document.getElementById("audio-player")
                await steckerConnection.listenForAudioChannel(htmlPlayer);
                this.steckerAudioChannelIn = steckerConnection;
                break;
            default:
                alert(`Unknown room type ${roomType}`);
                return;
        }

        let localDescription = await steckerConnection.generateLocalSessionDescription();

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
                    offer: localDescription,
                    roomType: roomType.toUpperCase(),
                },
            }),
        });
        let rawResponse = await results.json();
        let remoteSessionDescription = rawResponse.data.joinRoom;

        steckerConnection.peerConnection.setRemoteDescription(
            new RTCSessionDescription(JSON.parse(atob(remoteSessionDescription)))
        );
        this.connectedRoom = true;
    },
});

s = Alpine.store("stecker");
