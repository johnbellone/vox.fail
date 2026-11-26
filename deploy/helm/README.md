# Vox on Kubernetes (Helm)

`deploy/helm/vox` runs everything from one image (`deploy/docker/Dockerfile`): the directory,
one world server per world, and an edge in front. The single-machine setup with systemd and a
Cloudflare Tunnel is in [../README.md](../README.md); this does the same in a cluster.

```
            Ingress or Cloudflare Tunnel
                       |
                  edge (nginx, 2 pods)
       caches /pkg/ /assets/ /data/, refuses /api/world/
          |                          |
   /w/<port>/ws (WebSockets)      everything else
          |                          |
   world servers  --- report --->  directory
   (one StatefulSet each,          (StatefulSet, 1 replica,
    SQLite + snapshots on           SQLite + signing key on
    their own volume)               its volume)
```

- **directory**: accounts, characters, sign-in, the pages and the game's files. SQLite and the
  signing key are on its volume, so it is one replica.
- **worlds**: one StatefulSet per entry in `worlds:`, one replica each (a world is one process
  that owns its blocks). Each waits for the directory, fetches its public key from `/api/key`
  (an init container), and keeps SQLite and its snapshots (a week of them) on its own volume.
  With `worldDefaults.snapshots.oxen` (on by default; the image has the `oxen` command line) the
  snapshots are commits in an Oxen repository, and history older than a week is squashed away
  once a day. Its `port` is the number in its public path, `/w/<port>/ws`, which the levels' portals name
  (`":7781"`), so keep the numbers the levels use.
- **edge**: nginx. It caches the game's files per pod (the directory marks them cacheable for
  five minutes; the browser client is about 11 MB compressed), carries `/w/<port>/` to the
  worlds, and refuses `/api/world/` (the worlds use it inside the cluster). It passes the
  visitor's address on unchanged, so the directory's sign-in limits see the real person, and
  drops `CF-Connecting-IP` unless the Cloudflare Tunnel is on.
- **ingress** (optional): a Kubernetes Ingress to the edge.
- **cloudflared** (optional): a Cloudflare Tunnel to the edge, like vox.fail today.
- **NetworkPolicies**: only the edge reaches the worlds; only the edge and the worlds reach the
  directory.

Both servers save and stop on SIGTERM (worlds take a last snapshot), and the worlds get 60
seconds to do it.

## The image

```bash
docker build -f deploy/docker/Dockerfile -t ghcr.io/<you>/vox:0.1.0 .
docker push ghcr.io/<you>/vox:0.1.0
```

It holds both servers, `assets/`, `data/` (blocks and levels) and the built site with the
browser client. Building the browser client (Bevy for wasm32) is the slow part;
`--build-arg WEB=skip` leaves it out for a quick build (the play page then has nothing to load).

## Installing

```bash
helm install vox deploy/helm/vox -n vox --create-namespace \
  --set image.repository=ghcr.io/<you>/vox --set image.tag=0.1.0 \
  --set publicUrl=https://vox.fail \
  --set ingress.enabled=true --set ingress.host=vox.fail
```

Secrets: either let the chart make them (`secrets.discordClientId`, `secrets.discordClientSecret`,
`secrets.steamKey`; the world secret is made up once and kept across upgrades), or make your
own Secret with the keys `VOX_WORLD_SECRET`, `VOX_DISCORD_CLIENT_ID`,
`VOX_DISCORD_CLIENT_SECRET` and `VOX_STEAM_KEY` and set `secrets.existingSecret`.

With the Cloudflare Tunnel instead of an Ingress:

```bash
kubectl -n vox create secret generic cloudflared-token --from-literal=token=<tunnel token>
helm install vox deploy/helm/vox -n vox --set cloudflared.enabled=true ...
```

and in the dashboard give the tunnel one public hostname (vox.fail) pointing at
`http://vox-edge:8080`. The Cloudflare settings in [../README.md](../README.md) (WebSockets,
the cache rule for `/pkg/`, `/assets/`, `/data/`, Rocket Loader off) still apply.

To try it without either: install with `--set publicUrl=http://localhost:8080`, then
`kubectl -n vox port-forward svc/vox-edge 8080:8080` and open http://localhost:8080.

## Adding a world

Add an entry under `worlds:` (`id`, `name`, `port`, `seed`, and any of `worldDefaults`' keys to
override them) and `helm upgrade`. The directory learns of it, the edge routes it, and a new
world starts from its seed level. Levels come from the image (`data/levels/`).

## Operating

Make someone a world's owner, or give authority:

```bash
kubectl -n vox exec vox-directory-0 -- vox-directory --db /var/lib/vox/directory.db \
  --key /var/lib/vox/directory.key --world "void=The Void=/w/7780/ws" admin own-world void "Their Name"
```

A world's snapshots and a restore (the world must be stopped while restoring):

```bash
kubectl -n vox scale statefulset/vox-world-void --replicas=0
kubectl -n vox run restore --rm -it --image ghcr.io/<you>/vox:0.1.0 --overrides '{"spec":{"securityContext":{"runAsUser":10001,"fsGroup":10001},"volumes":[{"name":"d","persistentVolumeClaim":{"claimName":"data-vox-world-void-0"}}],"containers":[{"name":"r","image":"ghcr.io/<you>/vox:0.1.0","stdin":true,"tty":true,"command":["sh"],"volumeMounts":[{"name":"d","mountPath":"/var/lib/vox"}]}]}}'
#   vox-server --db /var/lib/vox/void.db --list-snapshots
#   vox-server --db /var/lib/vox/void.db --restore-at <unix time>
kubectl -n vox scale statefulset/vox-world-void --replicas=1
```

Back up the volumes: the directory's (accounts, the signing key, wallets and ledger) and every
world's (`data-vox-directory-0`, `data-vox-world-<id>-0`).

## Values

See [vox/values.yaml](vox/values.yaml): every value is commented. The ones most often set:
`image.*`, `publicUrl`, `secrets.*`, `worlds`, `worldDefaults` (players, guests, snapshots,
volume size), `edge.cache.*`, `ingress.*`, `cloudflared.*`.
