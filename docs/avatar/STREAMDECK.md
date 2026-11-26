# Stream Deck buttons

Every control on the control page is also a plain web address, so a Stream Deck (or anything that
can send a web request: Touch Portal, Loupedeck, a phone shortcut) can press it.

**Set up a button:** drag the **Website** action (System category) onto a key, paste one of the
addresses below as the URL, and tick **GET request in background** so it doesn't open a browser
tab. If you changed the control page's port, use that instead of 7777.

## Scenes and the show

| Button | Address |
| --- | --- |
| Load a saved scene | `http://127.0.0.1:7777/cmd/preset:load:Late%20show` |
| Next set | `http://127.0.0.1:7777/cmd/set:next` |
| Diner / night / rally | `http://127.0.0.1:7777/cmd/set:diner` (or `night`, `rally`, `newsroom`, `pressroom`, `hospital`, `none`) |
| Framing: head / waist / full | `http://127.0.0.1:7777/cmd/frame:0` (`1`, `2`) |
| Background: green / studio | `http://127.0.0.1:7777/cmd/bg:green` (`magenta`, `studio`, `transparent`) |
| Next character | `http://127.0.0.1:7777/cmd/model:next` |
| Press conference | `http://127.0.0.1:7777/cmd/press` |

## Moments

| Button | Address |
| --- | --- |
| Clip that (last 20 s) | `http://127.0.0.1:7777/cmd/clip` |
| Highlight reel | `http://127.0.0.1:7777/cmd/clip:reel` |
| Fireworks | `http://127.0.0.1:7777/cmd/fx:fireworks` |
| Confetti cannons | `http://127.0.0.1:7777/cmd/fx:confetti` |
| Hearts | `http://127.0.0.1:7777/cmd/fx:hearts` |
| Rain / snow / weather off | `http://127.0.0.1:7777/cmd/fx:rain` (`snow`, `dust`, `drift`, `auto`, `off`) |
| Throw a tomato | `http://127.0.0.1:7777/cmd/throw:tomato` |
| Poll (60 s) | `http://127.0.0.1:7777/cmd/poll:start:60:Best%20snack%3F%7CPizza%7CTacos` |
| End the poll | `http://127.0.0.1:7777/cmd/poll:end` |

## Face and voice

| Button | Address |
| --- | --- |
| Face 1-10 | `http://127.0.0.1:7777/cmd/face:0` ... `face:9` |
| Emote | `http://127.0.0.1:7777/cmd/emote:nod` (`shake`, `tilt`, `hop`, `spin`, `bow`, `shiver`, `lean`) |
| Mute the mic | `http://127.0.0.1:7777/cmd/mute` |
| Face capture on/off | `http://127.0.0.1:7777/cmd/facecap:on` |

## Viewer questions

| Button | Address |
| --- | --- |
| Air the next question | `http://127.0.0.1:7777/cmd/ask:next` |
| Done (thanks) | `http://127.0.0.1:7777/cmd/ask:done` |
| Cut | `http://127.0.0.1:7777/cmd/ask:cut` |

Spaces and symbols in names go in as `%20` (space), `%3F` (?) and `%7C` (|). The control page
only answers from this computer; the requests above come from the Stream Deck software on the
same machine, so nothing else needs opening up.
