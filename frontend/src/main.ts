import { User } from "./types";
import * as utils from "./utils";
import * as ws from "./wasm/crypto_wasm.js";
import { messageStore } from "./store";

const PROFILE_KEY = "profile";

const welcome_dialog = document.getElementById("welcome_dialog") as HTMLDialogElement | null;
const messages = document.getElementById("messages");
const message_form = document.getElementById("message_form") as HTMLFormElement | null;

// Global state
let socket: WebSocket | null = null;
let profile: User | null = null;
let groupId: string | null = null;
const users: { [id: string]: User } = {};

function append_user_message(name: string, text: string): void {
    const color = utils.name_color(name);
    if (messages) {
        messages.innerHTML += `
        <div class="user-message">
            <b style="color:${color}">${name}: </b>
            ${text}
        </div>
        `;
    }
}

function append_server_message(text: string): void {
    if (messages) {
        messages.innerHTML += `
            <div class="server-message">${text}</div>
        `;
    }
}

async function load_stored_messages() {
    if (messages) messages.innerHTML = "";

    const msgs = await messageStore.getMessagesByGroupId(groupId);
    console.log("Stored messages", {
        groupId,
        msgs
    });

    msgs.forEach(({ sender, payload }) => {
        append_user_message(sender, payload);
    });
}

function on_message(event: MessageEvent): void {
    if (profile == null) return;

    const msg = JSON.parse(event.data);
    console.log(msg);

    switch (msg.kind) {
        case "new_user":
            users[msg.user.id] = msg.user;
            append_server_message(`${msg.user.name} joined the chat.`);
            break;
        case "relay_message":
            const user = users[msg.sender];
            const text = ws.decrypt_message(msg.payload, profile.private_key);

            append_user_message(user.name, text);
            messageStore.appendMessage({
                sender: user.name,
                payload: text,
                groupId
            });
            break;
        case "user_left":
            delete users[msg.user_id];
            // const name = users[msg.user_id].name;
            // append_server_message(`${name} left the chat.`);
            break;
    }
}

function ws_setup() {
    socket = new WebSocket("/ws");

    socket.onopen = () => {
        if (socket == null) return;
        if (profile == null) return;
        if (message_form == null) return;

        socket.send(JSON.stringify({
            kind: "first",
            public_key: profile.public_key,
            name: profile.name
        }));

        message_form.addEventListener("submit", (event: SubmitEvent) => {
            event.preventDefault();

            if (event.target == null) return;
            if (profile == null) return;

            const text = message_form.getElementsByTagName("input")[0].value;
            append_user_message(profile.name, text);
            messageStore.appendMessage({
                sender: profile.name,
                payload: text,
                groupId
            });

            Object.keys(users).forEach(user_id => {
                const user = users[user_id];
                const payload = ws.encrypt_message(text, user.public_key);
                if (socket) {
                    socket.send(JSON.stringify({
                        kind: "send_message",
                        recipient: user_id,
                        payload
                    }));
                }
            });

            (event.target as HTMLFormElement).reset();
        });
    };

    socket.onmessage = on_message;

    groupId = null;
    load_stored_messages();
}

function on_welcome(event: SubmitEvent) {
    if (event.target == null) return;
    if (profile == null) return;
    const welcome_form = event.target as HTMLFormElement;

    profile.name = welcome_form.getElementsByTagName("input")[0].value;
    localStorage.setItem(PROFILE_KEY, JSON.stringify(profile));

    ws_setup();

    append_server_message(`${profile.name} joined the chat.`);
}

messageStore.init().then(() => {
    const profile_json = localStorage.getItem(PROFILE_KEY);
    if (profile_json) {
        profile = JSON.parse(profile_json);
        ws_setup();
    } else if (welcome_dialog) {
        const { public_key, private_key } = JSON.parse(ws.generate_keypair()) as User;
        profile = {
            id: public_key,
            name: "John Doe",
            public_key,
            private_key
        }
        const form = welcome_dialog.getElementsByTagName("form")[0];
        form.addEventListener("submit", on_welcome);
        welcome_dialog.showModal();
    }
});
