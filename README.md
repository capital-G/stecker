# Stecker

> A digital patch bay for audio signals.

Stecker is a framework to distribute and receive low latency audio and control signals over the internet using SuperCollider or on a client with a Browser.

Stecker consists of a server which handles the distribution of signals to its clients and offers an HTML frontend which is accessible from a browser and a client implementation of Stecker called *SuperStecker*, which allows to send and receive those signals within the SuperCollider server.

Service | Directory | Comment
--- | --- | ---
server | `src/server` | Stecker server
SuperStecker | `src/sc_plugin` | SuperCollider UGens to send and receive signals from Stecker
client | `src/client` | A command line client for debugging
steckerlib | `src/shared` | Library for shared functionality between client and server

## Server

> A public server is available under [`stecker.dennis-scheiba.com`](https://stecker.dennis-scheiba.com)

To start a server locally run e.g.

```shell
cargo run --bin server -- --port 8000
```

The HTML frontend of the server is accessible via [`http://localhost:8000/`](http://localhost:8000/).

### Expose a server to the internet

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

## Implementation

Stecker uses WebRTC to create a peer-to-peer connection between a client and another client (where one client of this connection would be a Stecker server).
As the WebRTC standard does not include an implementation to exchange the necessary data for such a connection build, Stecker uses GraphQL to exchange the necessary information.

The GraphQL API is accessible via [`http://localhost:8000/graphql`](http://localhost:8000/graphql).

## License

AGPL-3.0
