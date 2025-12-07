import init, { 
    generate_keypair,
    encrypt_message,
    decrypt_message 
} from "/crypto_wasm.js";

const welcome = document.getElementById("welcome_dialog");
const messages = document.getElementById("messages");
const form = document.getElementById("message_form");

let socket, my_name, my_keys;
const users = {};

function nameColor(str) {
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
        hash = str.charCodeAt(i) + ((hash << 5) - hash);
    }
    const hue = hash % 360;
    const saturation = 85;
    const lightness = 60;
    const h = hue / 360;
    const s = saturation / 100;
    const l = lightness / 100;
    let r, g, b;
    if (s === 0) {
        r = g = b = l;
    } else {
        const hue2rgb = (p, q, t) => {
            if (t < 0) t += 1;
            if (t > 1) t -= 1;
            if (t < 1/6) return p + (q - p) * 6 * t;
            if (t < 1/2) return q;
            if (t < 2/3) return p + (q - p) * (2/3 - t) * 6;
            return p;
        };
        const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
        const p = 2 * l - q;
        r = hue2rgb(p, q, h + 1/3);
        g = hue2rgb(p, q, h);
        b = hue2rgb(p, q, h - 1/3);
    }
    const toHex = (c) => {
        const hex = Math.round(c * 255).toString(16);
        return hex.length === 1 ? '0' + hex : hex;
    };
    const rHex = toHex(r);
    const gHex = toHex(g);
    const bHex = toHex(b);
    return `#${rHex}${gHex}${bHex}`.toUpperCase();
}

function append_user_message(name, text) {
    const color = nameColor(name);
    messages.innerHTML += `<div class="user-message">
            <b style="color:${color}">${name}: </b>
            ${text}
        </div>`;
}

function append_server_message(text) {
    messages.innerHTML += `<div class="server-message">${text}</div>`;
}

function onmessage(event) {
    const msg = JSON.parse(event.data);
    console.log(msg);

    switch(msg.kind) {
        case "new_user":
            users[msg.user.id] = msg.user;
            append_server_message(`${msg.user.name} joined the chat.`);
            break;
        case "relay_message":
            const user = users[msg.sender];
            const text = decrypt_message(msg.payload, my_keys.private_key);
            append_user_message(user.name, text);
            break;
    }
}

function onwelcome(event) {
    my_name = event.target.getElementsByTagName("input")[0].value;
    socket = new WebSocket("/ws");

    socket.onopen = function() {
        socket.send(JSON.stringify({
            kind: "first",
            public_key: my_keys.public_key,
            name: my_name
        }));

        form.addEventListener("submit", (event) => {
            event.preventDefault();
            const text = event.target.getElementsByTagName("input")[0].value;

            append_user_message(my_name, text);

            Object.keys(users).forEach(user_id => {
                const user = users[user_id];
                const payload = encrypt_message(text, user.public_key);
                socket.send(JSON.stringify({
                    kind: "send_message",
                    recipient: user_id,
                    payload
                }));
            });

            event.target.reset();
        });
    };

    socket.onmessage = onmessage;

    append_server_message(`${my_name} joined the chat.`);
}

init().then(function() {
    my_keys = JSON.parse(generate_keypair());

    const form = welcome_dialog.getElementsByTagName("form")[0];
    form.addEventListener("submit", onwelcome);

    welcome.showModal();
});
