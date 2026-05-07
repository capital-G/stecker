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
            Alpine.store("stecker").isPlaying = true;
            let stream = event.streams[0];
            if (!stream) {
                stream = new MediaStream([event.track]);
            }
            htmlPlayer.srcObject = stream;
            htmlPlayer.autoplay = true;
            htmlPlayer.controls = true;
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
     * @param {string} roomType - one of "FLOAT", "CHAT" or "META"
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
                case "FLOAT":
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
            case "FLOAT":
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
    steckerMetaChannel: null,
    steckerFloatChannel: null,
    steckerChatChannel: null,

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
    isConnected: false,
    isPlaying: false,

    /**
     *
     * @returns {void}
     */
    async getRooms() {
        let results = await fetch(this.HOST, {
            method: "POST",

            headers: {
                "Content-Type": "application/json",
            },

            body: JSON.stringify({
                query: `
                    query getRooms {
                    rooms {
                        name,
                        uuid,
                        numListeners,
                        description,
                        floatChannel,
                        chatChannel,
                        audioChannel
                    }
                    }
                `,
                variables: {},
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
     * @param {string} name
     * @param {string} channelType - one of "AUDIO", "FLOAT", "STRING"
     * @returns {void}
     */
    async createRoom(name, channelType) {
        let steckerConnection = new SteckerConnection();

        this.steckerMetaChannel = new SteckerDataChannel(steckerConnection, "meta", (msg) => {
            this.log(`META(${name}): ${msg}`);
        });

        switch (channelType) {
            case "AUDIO":
                await steckerConnection.createAudioChannel();
                break;
            case "FLOAT":
                this.steckerFloatChannel = new SteckerDataChannel(steckerConnection, "FLOAT", (msg) => {
                    this.floatValue = msg;
                });
                this.allowSendFloat = true;
                break;
            case "STRING":
                this.steckerChatChannel = new SteckerDataChannel(steckerConnection, "STRING", (msg) => {
                    this.log(`CHAT: ${msg}`);
                });
                this.allowSendChat = true;
                break;
        }

        let localSessionDescription = await steckerConnection.generateLocalSessionDescription();

        let response = await fetch(this.HOST, {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
            },
            body: JSON.stringify({
                query: `
                    mutation createRoom($name:String!, $offer:String!, $channelType:ChannelType!, $password:String, $description:String) {
                        createRoom(name: $name, offer:$offer, channelType: $channelType, password:$password, description: $description) {
                            password,
                            offer,
                        }
                    }
                `,
                variables: {
                    name: name,
                    offer: localSessionDescription,
                    channelType: channelType,
                    password: null,
                    description: null,
                },
            }),
        });
        if (!response.ok) {
            alert(`Error during room creation: ${await response.text()}`);
            return;
        }
        let jsonResponse = await response.json();
        console.log(`JSON response from API: `, jsonResponse);
        if (jsonResponse.errors !== undefined) {
            alert(`Error during room creation: ${JSON.stringify(jsonResponse.errors)}`);
            return;
        }

        let remoteSessionDescription = jsonResponse.data.createRoom.offer;
        await steckerConnection.peerConnection.setRemoteDescription(
            new RTCSessionDescription(JSON.parse(atob(remoteSessionDescription)))
        );
        this.createdRoom = true;
        this.isConnected = true;
    },

    log(message) {
        this.messages.push(message);
    },

    sendFloatValue() {
        this.steckerFloatChannel.sendValue(this.floatValue);
    },

    sendChatValue() {
        this.steckerChatChannel.sendValue(this.chatValue);
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
     * @param {string} name
     * @param {string} channelType - "AUDIO", "FLOAT", or "STRING"
     * @returns {Promise<void>}
     */
    async _sendJoinRoom(name, steckerConnection, channelType) {
        let localDescription = await steckerConnection.generateLocalSessionDescription();

        let results = await fetch(this.HOST, {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
            },
            body: JSON.stringify({
                query: `
                mutation joinRoom($name: String!, $offer: String!, $channelType: ChannelType!) {
                    joinRoom(name: $name, offer: $offer, channelType: $channelType)
                }
                `,
                variables: {
                    name,
                    offer: localDescription,
                    channelType,
                },
            }),
        });
        let rawResponse = await results.json();
        if (rawResponse.errors) {
            console.error(`Error joining ${channelType} channel:`, rawResponse.errors);
            return;
        }
        let remoteSessionDescription = rawResponse.data.joinRoom;

        await steckerConnection.peerConnection.setRemoteDescription(
            new RTCSessionDescription(JSON.parse(atob(remoteSessionDescription)))
        );
    },

    /**
     * @param {string} name
     * @param {Boolean} audioChannel
     * @param {Boolean} floatChannel
     * @param {Boolean} chatChannel
     * @param {string|null} returnRoomPrefix
     * @param {boolean} addRandomPostfix
     */
    async joinRoom(name, audioChannel, floatChannel, chatChannel, returnRoomPrefix, addRandomPostfix) {
        console.log(`Joining room: ${name}: audio ${audioChannel}, float: ${floatChannel}, chat: ${chatChannel}`);
        if(returnRoomPrefix != null) {
            let randomString = addRandomPostfix ? (Math.random() + 1).toString(36).substring(7) : '';
            let returnRoomName = `${returnRoomPrefix}${name}${randomString}`;
            console.log(`Create return room ${returnRoomName}`);
            this.createRoom(returnRoomName, "AUDIO");
        }

        if (audioChannel) {
            let steckerConnection = new SteckerConnection();
            new SteckerDataChannel(steckerConnection, "meta", (msg) => {
                this.log(`META(${name}): ${msg}`);
            });

            let htmlPlayer = document.getElementById("audio-player");
            await steckerConnection.listenForAudioChannel(htmlPlayer);
            this.steckerAudioChannelIn = steckerConnection;

            await this._sendJoinRoom(name, steckerConnection, "AUDIO");
        }

        if (floatChannel) {
            let steckerConnection = new SteckerConnection();
            this.steckerFloatChannel = new SteckerDataChannel(steckerConnection, "FLOAT", (msg) => {
                this.floatValue = msg;
            });

            await this._sendJoinRoom(name, steckerConnection, "FLOAT");
        }

        if (chatChannel) {
            let steckerConnection = new SteckerConnection();
            this.steckerChatChannel = new SteckerDataChannel(steckerConnection, "STRING", (msg) => {
                this.log(`Chat(${name}): ${msg}`);
            });

            await this._sendJoinRoom(name, steckerConnection, "STRING");
        }

        this.connectedRoom = true;
        this.isConnected = true;
    },
});

s = Alpine.store("stecker");
