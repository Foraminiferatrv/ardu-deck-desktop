import { useEffect, useRef, useState } from "react";
import twitchLogo from "./assets/icons/twitch.svg";
import discordLogo from "./assets/icons/discord.svg";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import { listen } from "@tauri-apps/api/event";

import CheckIcon from "./assets/icons/check.svg";
import CrossIcon from "./assets/icons/cross.svg";

type AppState = {
  followers: number,
  is_discord_auth: boolean,
  is_live: boolean,
  is_micro_on: boolean,
  is_twitch_auth: boolean,
  viewers: number
};

const defaultState: AppState = {
  followers: 0,
  is_discord_auth: false,
  is_live: false,
  is_micro_on: false,
  is_twitch_auth: false,
  viewers: 0
};

function App() {
  let unlisteners: (() => void)[] = [];

  const [isTwitchAuth, setIsTwitchAuth] = useState(false);
  const [appState, setAppState] = useState<AppState>(defaultState);

  let isInitRef = useRef(false);
  let isDiscordInitRef = useRef(false);

  async function handleAuthDiscord() {
    await invoke("init_discord")
  }

  async function updateAppState() {
    let state: AppState = await invoke("get_state");
    setAppState(state);
  }

  async function handleAuthTwitch() {
    await invoke("auth_twitch");
    await updateAppState();
  }

  useEffect(() => {
    async function listenTauri() {
      let unlisten_discord = await listen<boolean>("discord-auth", async () => {
        await updateAppState();
      });

      let unlisten_twitch = await listen<boolean>("twitch-auth", async () => {
        await updateAppState();
      });

      unlisteners.push(unlisten_discord);
      unlisteners.push(unlisten_twitch);
    }

    listenTauri();

    return () => {
      unlisteners.forEach((unlisten) => unlisten());
    }
  }, []);

  useEffect(() => {
    async function initDiscordStart() {
      await handleAuthDiscord();//Never returns
      await updateAppState();
    }

    if (!isDiscordInitRef.current && !appState.is_discord_auth) {
      isDiscordInitRef.current = true;
      initDiscordStart();
    }

  }, [appState.is_discord_auth]);

  useEffect(() => {
    if (!isInitRef.current) {
      isInitRef.current = true;
      invoke("init_deck")// Never returns
    }
  }, []);

  console.log({ appState })

  return (
    <main className="container">
      <h1 className="font-bold">Auth</h1>

      <div className="row gap-10">
        <div className="relative">
          <a target="_blank" onClick={handleAuthDiscord}>
            <img src={discordLogo} className="logo discord size-40 cursor-pointer" alt="Auth Discord" />
          </a>
          <div className="absolute bottom-5 left-4.5 bg-white size-4" />
          {appState.is_discord_auth ? <img className="absolute bottom-3 left-2" height={"36px"} width={"36px"} src={CheckIcon} alt="Authenticated" /> : <img className="absolute bottom-3 left-2 " src={CrossIcon} height={"36px"} width={"36px"} alt="Not Authenticated" />}
        </div>

        <div className="relative">
          <a target="_blank" onClick={handleAuthTwitch}>
            <img src={twitchLogo} className="logo twitch size-40 cursor-pointer" alt="Auth Twitch" />
            <div className="absolute bottom-5 left-4.5 bg-white size-4" />
            {appState.is_twitch_auth ? <img className="absolute bottom-3 left-2" height={"36px"} width={"36px"} src={CheckIcon} alt="Authenticated" /> : <img className="absolute bottom-3 left-2" src={CrossIcon} height={"36px"} width={"36px"} alt="Not Authenticated" />}
          </a>
        </div>
      </div>

    </main >
  );
}

export default App;
