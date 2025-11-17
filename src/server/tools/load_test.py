import asyncio
import json
import logging
from typing import Any, Dict, List
import base64
import sys

from aiohttp import ClientSession
from aiortc import RTCPeerConnection, RTCSessionDescription, RTCConfiguration, RTCIceServer, MediaStreamTrack

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("webrtc-client")


class WebRTCClient:
    def __init__(self, server_urL: str, room_name: str, client_id: int):
        self.client_id = client_id
        self.room_name = room_name
        self.server_url = server_urL
        self.graphql_endpoint = f"{self.server_url}/graphql"

        self.peer_connection = RTCPeerConnection(configuration=RTCConfiguration(
            iceServers=[RTCIceServer(urls=["stun:stun.l.google.com:19302"])],
        ))

        self.peer_connection.on("connectionstatechange", self._on_conn_state)
        self.peer_connection.on("iceconnectionstatechange", self._on_ice_state)
        self.peer_connection.on("track", self._on_track)

    def _log(self, msg: Any, *args: Any):
        logger.info(f"[client {self.client_id}] {msg}", *args)

    async def _on_conn_state(self):
        self._log("PeerConnection state: %s", self.peer_connection.connectionState)
        if self.peer_connection.connectionState in ("failed", "closed"):
            await self.close()

    async def _on_ice_state(self):
        self._log("ICE state: %s", self.peer_connection.iceConnectionState)

    def _on_track(self, track: MediaStreamTrack):
        self._log(f"Received track: kind={track.kind} id={track.id}")

    async def join_room(self, room_name: str, offer: RTCSessionDescription) -> Dict[str, Any]:
        offer_dict = {
            "sdp": offer.sdp,
            "type": offer.type
        }
        offer_string = json.dumps(offer_dict)
        payload: Any = {
            "query": "mutation foo($roomName: String!, $offer: String!) {joinRoom(name: $roomName, offer: $offer, roomType: AUDIO)}",
            "variables": {
                "roomName": "foo",
                "offer": base64.b64encode(offer_string.encode()).decode("utf-8"),
            },
            "operationName": "foo",
        }
        # print(payload)
        # payload = """{"query":"mutation foo($roomName: String!, $offer: String!) {\\n  joinRoom(name: $roomName, offer: $offer, roomType: AUDIO)\\n}","variables":{"roomName":"{}","offer":"eue"},"operationName":"foo"}"""
        async with ClientSession() as session:
            async with session.post(url=self.graphql_endpoint, json=payload) as resp:
                # print(await resp.text())
                return await resp.json()

    async def create_offer(self) -> RTCSessionDescription:
        audio_transceiver = self.peer_connection.addTransceiver('audio')

        await self.peer_connection.setLocalDescription(await self.peer_connection.createOffer())
        # answer = await self.peer_connection.createOffer()
        # await self.peer_connection.setLocalDescription()
        return self.peer_connection.localDescription


    async def negotiate(self):
        offer = await self.create_offer()
        response = await self.join_room(self.room_name, offer)

        responese_raw = response['data']['joinRoom']

        # answer = RTCSessionDescription(ans["sdp"], ans["type"])

        sdp_dict = json.loads(base64.b64decode(responese_raw))
        print(sdp_dict)

        await self.peer_connection.setRemoteDescription(RTCSessionDescription(sdp=sdp_dict['sdp'], type=sdp_dict['type']))

        self._log("Negotiation complete")

    async def start(self):
        self._log("Starting client")

    async def close(self):
        self._log("Closing client")
        await self.peer_connection.close()
        self._log("Client cleanup done")

    async def run(self, duration: float = 10):
        await self.negotiate()
        await self.start()

        # Remain connected for the test duration
        try:
            await asyncio.sleep(duration)
        except asyncio.CancelledError:
            pass

        await self.close()



async def run_many(host: str, room_name:str, n: int=20):
    clients: List[asyncio.Task] = []
    for i in range(n):
        client = WebRTCClient(host, room_name, i)
        task = asyncio.create_task(client.run(duration=20))
        clients.append(task)

        await asyncio.sleep(0.05)

    await asyncio.gather(*clients)

if __name__ == "__main__":
    host = sys.argv[1]
    room_name = sys.argv[2]
    num = int(sys.argv[3])
    asyncio.run(run_many(host, room_name, num))
