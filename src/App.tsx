import { useEffect, useRef, useState } from "react";
import twitchLogo from "./assets/icons/twitch.svg";
import discordLogo from "./assets/icons/discord.svg";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import { listen } from "@tauri-apps/api/event";

import CheckIcon from "./assets/icons/check.svg";
import CrossIcon from "./assets/icons/cross.svg";
import { app } from "@tauri-apps/api";

function App() {
  // const [greetMsg, setGreetMsg] = useState("");
  // const [name, setName] = useState("");
  // const appState = app.
  const [isDiscordAuth, setIsDiscordAuth] = useState(false);
  const [isTwitchAuth, setIsTwitchAuth] = useState(false);

  let isInitRef = useRef(false);
  // console.log("init")
  async function handleAuthDiscord() {
    console.log("handleAuthDiscord")
    await invoke("init_discord")
  }

  async function handleAuthTwitch() {
    console.log("handleAuthTwitch")
    await invoke("auth_twitch")
  }


  useEffect(() => {
    async function initDeck() {
      if (!isInitRef.current) {
        console.log("Init")
        await invoke("init_deck")
        await handleAuthDiscord();
        isInitRef.current = true;
      }
    }

    listen<boolean>("discord-auth", (event) => {
      setIsDiscordAuth(event.payload);
    });

    initDeck();
  }, []);



  return (
    <main className="container">
      <h1>Auth</h1>

      <div className="row gap-10">
        <div className="relative">
          <a target="_blank" onClick={handleAuthDiscord}>
            <img src={discordLogo} className="logo discord size-40 cursor-pointer" alt="Auth Discord" />
          </a>
          <div className="absolute bottom-5 left-4.5 bg-white size-4" />
          {isDiscordAuth ? <img className="absolute bottom-3 left-2" height={"36px"} width={"36px"} src={CheckIcon} alt="Authenticated" /> : <img className="absolute bottom-3 left-2 " src={CrossIcon} height={"36px"} width={"36px"} alt="Not Authenticated" />}
        </div>

        <div className="relative">
          <a target="_blank" onClick={handleAuthTwitch}>
            <img src={twitchLogo} className="logo twitch size-40 cursor-pointer" alt="Auth Twitch" />
            <div className="absolute bottom-5 left-4.5 bg-white size-4" />
            {isTwitchAuth ? <img className="absolute bottom-3 left-2" height={"36px"} width={"36px"} src={CheckIcon} alt="Authenticated" /> : <img className="absolute bottom-3 left-2" src={CrossIcon} height={"36px"} width={"36px"} alt="Not Authenticated" />}
          </a>
        </div>
      </div>

      {/*<form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          // greet();
        }}
      >
        <input
          id="greet-input"
          // onChange={(e) => setName(e.currentTarget.value)}
          placeholder="Enter a name..."
        />
        <button type="submit">Greet</button>
      </form>*/}
      {/*<p>{greetMsg}</p>*/}
    </main >
  );
}

export default App;
