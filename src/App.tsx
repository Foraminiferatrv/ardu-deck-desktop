import { useEffect, useRef } from "react";
import twitchLogo from "./assets/icons/twitch.svg";
import discordLogo from "./assets/icons/discord.svg";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

function App() {
  // const [greetMsg, setGreetMsg] = useState("");
  // const [name, setName] = useState("");

  let isInitRef = useRef(false);

  useEffect(() => {
    async function initDeck() {
      if (!isInitRef.current) {

        await invoke("init_deck")
        isInitRef.current = true;
      }
    }

    initDeck();
  }, []);

  async function handleAuthDiscord() {
    await invoke("listen_discord_mic")
  }

  async function handleAuthTwitch() {
    await invoke("auth_twitch")
  }


  return (
    <main className="container">
      <h1>Auth</h1>

      <div className="row">
        <a target="_blank" onClick={handleAuthDiscord}>
          <img src={discordLogo} className="logo vite" alt="Auth Discord" />
        </a>

        <a target="_blank" onClick={handleAuthTwitch}>
          <img src={twitchLogo} className="logo react" alt="Auth Twitch" />
        </a>
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
    </main>
  );
}

export default App;
