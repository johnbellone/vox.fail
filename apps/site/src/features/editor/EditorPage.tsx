// The world editor page (/edit?world=<id>). It signs in as /play does, asks the directory whether
// this account may use Advanced controls, starts the WebAssembly client in edit mode on the
// canvas, and lays out the tools, the inspector and the entity list around it. The client's
// events are installed before the client starts, so none is missed.

import { Alert, Box, Button, Center, Flex, Group, Stack, Text } from "@mantine/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, ApiError, messageOf, post, worldUrl } from "../../api/client";
import { useApi } from "../../api/hooks";
import type { Inventory, Me, Token, WorldEntry, Worlds } from "../../api/types";
import { loadClient } from "../../game/wasm";
import { type Command, type NodePreview, type Tool, onEvent, sendCommand } from "../../game/editor";
import { chooseBlock } from "./BlockPalette";
import { EditorHeader, type Mode } from "./EditorHeader";
import { EntityList } from "./EntityList";
import { Inspector } from "./Inspector";
import { chooseProp, useCatalogue } from "./PropCatalogue";
import { NewNode } from "./NewNode";
import { NewPortal } from "./NewPortal";
import { PlaceProp } from "./PlaceProp";
import { ObjectivePanel, SpawnPanel } from "./SpawnObjective";
import { SettingsPanel } from "./SettingsPanel";
import {
  applyChanges,
  cleared,
  dropEdit,
  type Edits,
  kindOf,
  type PanelKind,
  propBytes,
  propNameOf,
  type PropSpot,
  refuseEdit,
  resendEdit,
  retryCommand,
  rowOfKind,
  type RowOf,
  type Entities,
  sentEdit,
  settleEdit,
  sortedRows,
  type Spot,
} from "./state";
import { ToolRail } from "./ToolRail";

const CANVAS = "editor-game";

export function EditorPage() {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [world, setWorld] = useState<WorldEntry>();
  const [status, setStatus] = useState("Opening the editor…");
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string>();
  const [notice, setNotice] = useState<string>();
  // the latest refusal of an edit; cleared by the next `done`, when an edit goes through
  const [refusal, setRefusal] = useState<string>();
  const [canAdvanced, setCanAdvanced] = useState(false);
  const [mode, setMode] = useState<Mode>("basic");
  const [tool, setTool] = useState<Tool>("select");
  const [entities, setEntities] = useState<Entities>(cleared);
  const [selected, setSelected] = useState<number | null>(null);
  const [here, setHere] = useState(0);
  /** what the client says the selected node would build */
  const [nodePreview, setNodePreview] = useState<NodePreview>();
  // the world's block names (from `ready`) and the one the page has chosen to paint with
  const [palette, setPalette] = useState<string[]>([]);
  const [block, setBlock] = useState<string>();
  // the catalogue prop the Prop tool places, by hash
  const [prop, setProp] = useState<string>();
  // the open portal edits, by ref (see state.ts); refs are numbered here and never reused
  const [edits, setEdits] = useState<Edits>(() => new Map());
  const editsRef = useRef(edits);
  useEffect(() => {
    editsRef.current = edits;
  }, [edits]);
  const nextRef = useRef(1);
  // stable, so the new-portal form's close effect runs when its edit settles, not on every render
  const closeSpot = useCallback(() => setSpot(undefined), []);
  // where the Prop tool last marked a spot: a place-prop form opens there, keyed by its number
  const [propSpot, setPropSpot] = useState<{ id: number; spot: PropSpot }>();
  const closePropSpot = useCallback(() => setPropSpot(undefined), []);
  // the catalogue of accepted props, loaded once for the rail, the list and the inspector
  const catalogue = useCatalogue();
  // where the Portal tool last marked a spot: a new-portal form opens there, keyed by its number
  const [spot, setSpot] = useState<{ id: number; spot: Spot }>();
  const spotSeq = useRef(0);
  // the advanced panel opened by a button (World settings, Add spawn, Add objective) with nothing
  // selected; a selected Spawn, Objective or Settings entity shows its own panel instead
  const [panelOpen, setPanelOpen] = useState<PanelKind>();
  // the Add node form, opened by its button with nothing selected (Advanced only)
  const [nodeOpen, setNodeOpen] = useState(false);
  const closeNode = useCallback(() => setNodeOpen(false), []);
  // the worlds to lead to, and the player's keys to pair with (both for the portal forms)
  const worldList = useApi<Worlds>("/api/worlds");
  const inventory = useApi<Inventory>("/api/inventory");
  const worlds: WorldEntry[] = worldList.data?.worlds ?? [];
  const items = inventory.data?.items ?? [];
  const itemsError = inventory.error?.message;

  useEffect(() => {
    let cancelled = false;

    // installed first, so the client's first events (sent as it starts) are not lost
    const off = onEvent((e) => {
      switch (e.type) {
        case "ready":
          // a new world: nothing from the last one (its entities, its tombstones) still holds
          setEntities(cleared());
          setSelected(null);
          setRefusal(undefined);
          setNotice(undefined);
          setEdits(new Map());
          setSpot(undefined);
          setPropSpot(undefined);
          setPanelOpen(undefined);
          setNodeOpen(false);
          setNodePreview(undefined);
          setPalette(e.palette);
          setBlock(undefined);
          setProp(undefined);
          // a new world opens on the Select tool: the page and the client both go back to it, so the
          // rail's highlight and the client's tool agree
          setTool("select");
          try {
            sendCommand({ type: "tool", tool: "select" });
          } catch (err) {
            setNotice(messageOf(err));
          }
          break;
        case "entities":
          setEntities((prev) => applyChanges(prev, e.changes));
          break;
        case "selected":
          setSelected(e.entity);
          // selecting an entity shows its own panel, in place of a panel opened by a button
          if (e.entity !== null) {
            setPanelOpen(undefined);
            setNodeOpen(false);
          }
          break;
        case "refused":
          // a refusal of an open portal edit is shown by its form; any other goes in the banner
          if (e.ref === null || !editsRef.current.has(e.ref)) setRefusal(e.text);
          setEdits((prev) => settleEdit(prev, e));
          break;
        case "done":
          setRefusal(undefined);
          setEdits((prev) => settleEdit(prev, e));
          break;
        case "spot":
          spotSeq.current += 1;
          setSpot({ id: spotSeq.current, spot: { at: e.at, axis: e.axis, size: e.size } });
          setPropSpot(undefined);
          break;
        case "prop_spot":
          spotSeq.current += 1;
          setPropSpot({ id: spotSeq.current, spot: { prop: e.prop, at: e.at, yaw: e.yaw } });
          setSpot(undefined);
          break;
        case "here":
          setHere(e.players.length);
          break;
        case "node_preview":
          setNodePreview({ node: e.node, blocks: e.blocks, tooBig: e.too_big });
          break;
        default:
          break;
      }
    });
    window.vox_status = (text) => setStatus(text);
    window.vox_ready = () => {
      setReady(true);
      canvas.current?.focus();
    };
    window.vox_error = (text) => setError(text);

    const checkAdvanced = async (id: string) => {
      try {
        await api(`/api/worlds/editors?world=${encodeURIComponent(id)}`);
        if (!cancelled) setCanAdvanced(true);
      } catch (err) {
        // 403 is the normal answer for someone who may only use the basic tools
        if (err instanceof ApiError && err.status === 403) return;
        console.error(err);
        if (!cancelled) setNotice(`Could not check your editing rights: ${messageOf(err)}`);
      }
    };

    (async () => {
      const me = await api<Me>("/api/me");
      if (!me.name) return void (location.href = "/");
      if (!me.character) return void (location.href = "/create");
      const want = new URLSearchParams(location.search).get("world") ?? "";
      const t = await post<Token>("/api/token");
      // the id ("yard") is what links carry; a name ("The Yard") is accepted for links made by other pages
      const found = t.worlds.find((w) => w.id === want) ?? t.worlds.find((w) => w.name === want);
      if (!found) throw new Error(want ? `there is no world called ${want}` : "name the world to edit, as /edit?world=yard");
      if (cancelled) return;
      setWorld(found);
      document.title = `Edit ${found.name} · Vox`;
      void checkAdvanced(found.id);
      setStatus("Loading the client…");
      const start = await loadClient();
      if (cancelled) return;
      setStatus(`Opening ${found.name}…`);
      start({
        canvas: `#${CANVAS}`,
        url: worldUrl(found.url),
        token: t.token,
        name: t.name,
        model: t.character.model,
        wear: t.character.wear,
        edit: true,
      });
    })().catch((err) => {
      if (!cancelled) setError(messageOf(err));
    });

    return () => {
      cancelled = true;
      off();
      delete window.vox_status;
      delete window.vox_ready;
      delete window.vox_error;
    };
  }, []);

  // sends a command to the client; a failure (no editor running) is shown like a refusal
  const send = (c: Command) => {
    try {
      sendCommand(c);
    } catch (err) {
      setNotice(messageOf(err));
    }
  };

  // sends one portal edit; a failure (no editor running) is recorded as its refusal
  const sendEdit = (ref: number, c: Command) => {
    try {
      sendCommand(c);
    } catch (err) {
      setEdits((prev) => refuseEdit(prev, ref, "NotRunning", messageOf(err)));
    }
  };

  // starts an edit: a fresh ref, the command built with it, sent. The caller's form follows the ref.
  const submit = (make: (ref: number) => Command): number => {
    const ref = nextRef.current;
    nextRef.current += 1;
    const c = make(ref);
    // set here, not only in the effect: a retry or a second submit in the same tick reads it
    const next = sentEdit(editsRef.current, ref, c);
    editsRef.current = next;
    setEdits(next);
    sendEdit(ref, c);
    return ref;
  };

  // resends a refused-Unavailable edit: the same command with the same ref (never charged twice)
  const retry = (ref: number) => {
    const c = retryCommand(editsRef.current, ref);
    if (!c) return;
    setEdits((prev) => resendEdit(prev, ref));
    sendEdit(ref, c);
  };

  const forget = (ref: number) => setEdits((prev) => dropEdit(prev, ref));

  const pickTool = (t: Tool) => {
    setTool(t);
    send({ type: "tool", tool: t });
  };

  // choosing a block paints with it: the Blocks tool is held, then the block command is sent
  const pickBlock = (name: string) => {
    setBlock(name);
    pickTool("blocks");
    send(chooseBlock(name));
  };

  // choosing a catalogue prop places it with the Prop tool: the prop is sent first, then the tool
  const pickProp = (hash: string) => {
    setProp(hash);
    send(chooseProp(hash));
    pickTool("prop");
  };

  // the Node tool is hidden in Basic, so leaving Advanced while it is active puts the Select tool back
  const changeMode = (m: Mode) => {
    setMode(m);
    if (m === "basic" && tool === "node") pickTool("select");
  };

  const rows = sortedRows(entities);
  const isAdvanced = canAdvanced && mode === "advanced";
  // the advanced panels: a selected Spawn, Objective or Settings entity shows its own; a panel
  // opened by a button shows when nothing is selected; anything else shows the Inspector
  const selectedRow = selected === null ? undefined : entities.rows.get(selected);
  const selectedKind = selectedRow ? kindOf(selectedRow.component) : undefined;
  let panel: PanelKind | undefined;
  if (isAdvanced) {
    if (selectedKind === undefined) panel = panelOpen;
    else if (selectedKind === "Spawn" || selectedKind === "Objective" || selectedKind === "Settings") panel = selectedKind;
  }
  // a selected row is the one edited (a world may have several spawns); a button edits the first
  const panelRow = <K extends PanelKind>(kind: K): RowOf<K> | undefined =>
    selectedRow !== undefined && kind in selectedRow.component ? (selectedRow as RowOf<K>) : rowOfKind(entities, kind);
  // the Add node form shows when a button opened it and nothing is selected
  const showNode = isAdvanced && nodeOpen && selectedKind === undefined;
  const spawnRow = panelRow("Spawn");
  const objectiveRow = panelRow("Objective");
  const settingsRow = panelRow("Settings");
  // a button opens its panel with nothing selected
  // the Add node form opens with nothing selected, in place of a button's panel
  const openNode = () => {
    setPanelOpen(undefined);
    setNodeOpen(true);
    setSelected(null);
    send({ type: "select", entity: null });
  };
  const openPanel = (kind: PanelKind) => {
    setNodeOpen(false);
    setPanelOpen(kind);
    setSelected(null);
    send({ type: "select", entity: null });
  };
  const closePanel = () => setPanelOpen(undefined);

  return (
    <Stack gap={0} mih="100dvh" style={{ maxWidth: "100%", overflowX: "hidden" }}>
      <EditorHeader
        world={world?.name ?? "…"}
        mode={canAdvanced ? mode : "basic"}
        onMode={changeMode}
        canAdvanced={canAdvanced}
        here={here}
        onUndo={() => send({ type: "undo" })}
        onRedo={() => send({ type: "redo" })}
      />
      {notice && (
        <Alert color="ember" role="alert" mx="sm" withCloseButton onClose={() => setNotice(undefined)}>
          {notice}
        </Alert>
      )}
      {refusal && (
        <Alert color="ember" role="alert" mx="sm" withCloseButton onClose={() => setRefusal(undefined)}>
          {refusal}
        </Alert>
      )}
      {error && ready && (
        <Alert color="ember" role="alert" mx="sm">
          The client reported an error: {error}
        </Alert>
      )}
      <Flex direction={{ base: "column", md: "row" }} flex={1} style={{ minHeight: 0 }}>
        <ToolRail
          active={tool}
          onTool={pickTool}
          advanced={canAdvanced && mode === "advanced"}
          palette={palette}
          block={block}
          onBlock={pickBlock}
          prop={prop}
          onProp={pickProp}
          catalogue={catalogue}
        />
        <Box flex={1} mih={420} pos="relative" style={{ minWidth: 0 }}>
          <canvas
            id={CANVAS}
            ref={canvas}
            tabIndex={0}
            aria-label="The world being edited"
            style={{ position: "absolute", inset: 0, width: "100%", height: "100%", display: "block", outline: "none", background: "#f3f5f9" }}
          />
          {spot && ready && world && (
            <Box pos="absolute" top={8} right={8} w="min(340px, calc(100% - 16px))" style={{ zIndex: 1, maxHeight: "calc(100% - 16px)", overflowY: "auto" }}>
              <NewPortal
                key={spot.id}
                spot={spot.spot}
                level={world.id}
                advanced={canAdvanced && mode === "advanced"}
                worlds={worlds}
                items={items}
                itemsError={itemsError}
                edits={edits}
                submit={submit}
                retry={retry}
                forget={forget}
                onClose={closeSpot}
              />
            </Box>
          )}
          {propSpot && ready && world && (
            <Box pos="absolute" top={8} right={8} w="min(340px, calc(100% - 16px))" style={{ zIndex: 1, maxHeight: "calc(100% - 16px)", overflowY: "auto" }}>
              <PlaceProp
                key={propSpot.id}
                spot={propSpot.spot}
                name={propNameOf(propBytes(propSpot.spot.prop), catalogue.entries ?? [])}
                advanced={canAdvanced && mode === "advanced"}
                edits={edits}
                submit={submit}
                retry={retry}
                forget={forget}
                onClose={closePropSpot}
              />
            </Box>
          )}
          {!ready && (
            <Center pos="absolute" inset={0} bg="#f3f5f9" aria-live="polite">
              <Stack align="center" gap="xs" w="min(360px, 80vw)">
                <Text c="moon.5">{error ?? status}</Text>
                {error && (
                  <Text component="a" href="/" fz="sm">
                    Back to the start
                  </Text>
                )}
              </Stack>
            </Center>
          )}
        </Box>
        {panel ? (
          <Stack gap="xs" p="sm" w={{ base: "100%", md: 280 }} style={{ flexShrink: 0 }}>
            {/* a panel opened by a button closes with its Close; one opened by a selected entity does not */}
            {panel === "Spawn" && (
              <SpawnPanel
                key={spawnRow?.entity ?? "new"}
                row={spawnRow}
                spot={spot?.spot}
                edits={edits}
                submit={submit}
                retry={retry}
                forget={forget}
                onClose={selectedKind === undefined ? closePanel : undefined}
              />
            )}
            {panel === "Objective" && (
              <ObjectivePanel
                key={objectiveRow?.entity ?? "new"}
                row={objectiveRow}
                spot={spot?.spot}
                edits={edits}
                submit={submit}
                retry={retry}
                forget={forget}
                onClose={selectedKind === undefined ? closePanel : undefined}
              />
            )}
            {panel === "Settings" && (
              <SettingsPanel
                key={settingsRow?.entity ?? "new"}
                row={settingsRow}
                palette={palette}
                edits={edits}
                submit={submit}
                retry={retry}
                forget={forget}
                onClose={selectedKind === undefined ? closePanel : undefined}
              />
            )}
          </Stack>
        ) : showNode ? (
          <Stack gap="xs" p="sm" w={{ base: "100%", md: 280 }} style={{ flexShrink: 0 }}>
            <NewNode
              spot={spot?.spot}
              edits={edits}
              submit={submit}
              retry={retry}
              forget={forget}
              onClose={closeNode}
            />
          </Stack>
        ) : (
          <Inspector
            row={selectedRow}
            nodePreview={nodePreview}
            catalogue={catalogue.entries ?? []}
            advanced={isAdvanced}
            worlds={worlds}
            items={items}
            itemsError={itemsError}
            edits={edits}
            submit={submit}
            retry={retry}
            forget={forget}
          />
        )}
      </Flex>
      {isAdvanced && (
        <Group px="sm" pt="xs" gap="xs">
          <Button size="xs" variant="default" onClick={() => openPanel("Settings")}>
            World settings
          </Button>
          {!objectiveRow && (
            <Button size="xs" variant="default" onClick={() => openPanel("Objective")}>
              Add objective
            </Button>
          )}
          {!spawnRow && (
            <Button size="xs" variant="default" onClick={() => openPanel("Spawn")}>
              Add spawn
            </Button>
          )}
          <Button size="xs" variant="default" onClick={openNode}>
            Add node
          </Button>
        </Group>
      )}
      <EntityList rows={rows} catalogue={catalogue.entries ?? []} selected={selected} onError={setNotice} />
    </Stack>
  );
}
