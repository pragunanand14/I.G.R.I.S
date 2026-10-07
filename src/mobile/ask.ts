import { useNavigate } from "react-router";
import { useChatStore } from "@/stores/chatStore";

/** Things the phone app can really do, phrased the way people ask. */
export const SUGGESTIONS = ["Remind me in 10 minutes to stretch", "What's my battery level?", "Which apps do I have?", "What's on my to-do list?"];

/** Start a new conversation with `text` and show it. */
export function useAsk() {
  const navigate = useNavigate();
  return (text: string) => {
    const chat = useChatStore.getState();
    void chat.openConversation(null);
    void navigate("/chat");
    return chat.send(text);
  };
}
