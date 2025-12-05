var welcome = document.getElementById("welcome_dialog");
var messages = document.getElementById("messages");
var form = document.getElementById("message_form");

welcome.showModal();

var socket;

function stringToHighContrastHexColor(str) {
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

function onmessage(event) {
    var msg = event.data;
    var match = msg.match(/(.*): (.*)/);

    if (match) {
        var color = stringToHighContrastHexColor(match[1]);
        messages.innerHTML += `<div class="user-message">
            <b style="color:${color}">${match[1]}: </b>
            ${match[2]}</div>`;
    } else {
        messages.innerHTML += `<div class="server-message">${msg}</div>`;
    }
}

function onwelcome(event) {
    var name = event.target.getElementsByTagName("input")[0].value;
    socket = new WebSocket("/ws");

    socket.onopen = function() {
        socket.send("IAM " + name);

        form.addEventListener("submit", function(event) {
            event.preventDefault();
            var msg = event.target.getElementsByTagName("input")[0].value;
            socket.send("MSG " + msg);
            event.target.reset();
        });
    };

    socket.onmessage = onmessage;
}
