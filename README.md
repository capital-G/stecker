# Stecker

> A digital patch bay for audio signals.

Stecker is a framework to distribute and receive low latency audio and control signals over the internet using SuperCollider or on a client with a Browser.

## Quickstart

> In order to use *Stecker* you need to have a *Stecker* server on the internet.
I currently provide an instance of such a server on <https://stecker.dennis-scheiba.com> which you can use but should not rely on.
You can see below on how to run your own server in the internet.

Assuming you have a stecker server at hand, you can install the Stecker UGen as an extension by copying the files into `Platform.userExtensionDir`.

Afterwards you can use Stecker as UGen like this:

```supercollider
// boot the server
s.boot;

// set the implicit stecker server address
Stecker.host = "https://stecker.dennis-scheiba.com";

// distribute audio by spawning a node which contains the SteckerOut UGen
(
Ndef(\x, {
	var sig = RLPF.ar(
		in: SinOscFB.ar(freq: 30.0, feedback: SinOscFB.ar(0.03, 3.3) * 2),
		freq: LFDNoise0.ar(5.0).exprange(100, 1000),
		rq: LFDNoise1.ar(1.5).range(0.01, 0.5),
	);
	SteckerOut.ar(
		input: sig,
		roomName: "myRoom",  // change me
	);
	sig;
});
)
// go on the website and join the `myRoom` channel - you should hear a sound.

// will stop transmission
Ndef(\x).clear;

// receive signal from stecker server
// assuming there is a room called `myOtherRoom`

(
Ndef(\x, {
	var sig = SteckerIn.ar(roomName: \myOtherRoom);
	sig;
}).play;
)

// stop receiving
Ndef(\x).clear;
```

## Development

Stecker consists of a server which handles the distribution of signals to its clients and offers an HTML frontend which is accessible from a browser and a client implementation of Stecker called *SuperStecker*, which allows to send and receive those signals within the SuperCollider server.

Service | Directory | Comment
--- | --- | ---
server | `src/server` | Stecker server
SuperStecker | `src/sc_plugin` | SuperCollider UGens to send and receive signals from Stecker
client | `src/client` | A command line client for debugging
steckerlib | `src/shared` | Library for shared functionality between client and server

### Server

> A public server is available under [`stecker.dennis-scheiba.com`](https://stecker.dennis-scheiba.com)

To start a server locally run e.g.

```shell
cargo run --bin server -- --port 8000
```

The HTML frontend of the server is accessible via [`http://localhost:8000/`](http://localhost:8000/).

#### Expose a server to the internet

If this server should be accessible via the internet it is necessary to wrap the connections via https to provide a secure context which is necessary for a browser to accept WebRTC signals.

Below is a basic reverse-proxy configuration for *nginx* which would to be made secure via *certbot*.

```nginx
server {
  server_name your-address.com;

  location / {
    proxy_pass  http://localhost:8000;
      proxy_set_header Host $host;
      proxy_set_header X-Real-IP $remote_addr;
      proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
      proxy_set_header X-Forwarded-Proto $scheme;
      proxy_set_header Upgrade $http_upgrade;
      proxy_set_header Connection "Upgrade";
  }
  listen 80
}
```

### Implementation

Stecker uses WebRTC to create a peer-to-peer connection between a client and another client (where one client of this connection would be a Stecker server).
As the WebRTC standard does not include an implementation to exchange the necessary data for such a connection build, Stecker uses GraphQL to exchange the necessary information.

The GraphQL API is accessible via [`http://localhost:8000/graphql`](http://localhost:8000/graphql).

## License

AGPL-3.0
