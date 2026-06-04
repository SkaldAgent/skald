# WhatsApp MCP Server (whatsapp)

## Overview

Un MCP server Node.js che espone WhatsApp come set di tool per l'LLM, usando **whatsapp-web.js** + Puppeteer (Chromium headless).

**Server name:** `whatsapp`  
**Transport:** `stdio` (spawna `node scripts/whatsapp_mcp/index.js`)  
**Location:** `scripts/whatsapp_mcp/index.js`

### Permessi

| Capability | Abilitato |
|------------|-----------|
| Listare chat e gruppi | ✅ |
| Leggere messaggi di una chat | ✅ |
| Cercare messaggi per parola chiave | ✅ |
| Cercare contatti per nome | ✅ |
| Inviare messaggi | ✅ |
| Scaricare media (foto, video, documenti) | ❌ non implementato |
| Modificare/cancellare messaggi | ❌ non implementato |

---

## ⚠️ Note importanti su WhatsApp

- **whatsapp-web.js è non ufficiale**: simula WhatsApp Web tramite Chromium. WhatsApp potrebbe bloccare il numero in caso di uso intensivo o anomalo.
- **Uso sicuro**: leggere i propri gruppi e inviare messaggi singoli è nella zona grigia tollerata. Non usare per spam o automazioni massive.
- **Numero consigliato**: usare un numero secondario o WhatsApp Business parallelo riduce il rischio.

---

## Tools

| Tool | Parametri | Descrizione |
|------|-----------|-------------|
| `whatsapp_status` | *(nessuno)* | Stato connessione: INITIALIZING, QR_READY, AUTHENTICATED, READY, DISCONNECTED |
| `whatsapp_get_qr` | *(nessuno)* | QR code ASCII da scansionare con il telefono (solo quando status = QR_READY) |
| `whatsapp_list_chats` | `max_chats` (int, default 20, max 50) | Lista chat recenti con nome, ID e conteggio messaggi non letti |
| `whatsapp_get_messages` | `chat_id` (required), `limit` (int, default 20, max 100), `offset` (int, default 0) | Messaggi di una chat/gruppo con supporto paginazione |
| `whatsapp_send_message` | `chat_id` (required), `message` (required) | Invia un messaggio di testo |
| `whatsapp_search_messages` | `query` (required), `max_results` (int, default 20, max 50) | Cerca per parola chiave in tutte le chat |
| `whatsapp_search_contacts` | `query` (required), `max_results` (int, default 20, max 50) | Cerca contatti salvati per nome (parziale, case-insensitive). Usare per trovare l'ID di un contatto non presente nelle chat recenti |

### Formato chat_id

- **Contatto:** `39xxxxxxxxxx@c.us` (prefisso internazionale senza `+`, seguito da `@c.us`)
- **Gruppo:** `xxxxxxxxxx-xxxxxxxxxx@g.us`

I chat_id corretti si ottengono tramite `whatsapp_list_chats` (chat recenti) o `whatsapp_search_contacts` (contatti salvati non in chat recenti).

---

## Autenticazione

### Prima volta (QR scan)

Al primo avvio non esiste una sessione salvata. Il client genera un QR code:

1. L'LLM chiama `whatsapp_status` → risposta `QR_READY`
2. L'LLM chiama `whatsapp_get_qr` → restituisce il QR ASCII
3. L'utente scansiona il QR con WhatsApp → **Impostazioni → Dispositivi collegati → Collega un dispositivo**
4. Lo stato passa a `AUTHENTICATED` poi `READY`

Il QR è salvato anche su file in `secrets/whatsapp_qr.txt`.

### Sessioni successive

La sessione viene persistita in `secrets/whatsapp_session/` (gestita da `LocalAuth` di whatsapp-web.js). Al riavvio del server la sessione viene ripristinata automaticamente, senza necessità di scansionare di nuovo il QR.

### Storage token

| File/Directory | Contenuto |
|---|---|
| `secrets/whatsapp_session/` | Sessione WhatsApp persistente (LocalAuth) |
| `secrets/whatsapp_qr.txt` | QR code temporaneo (eliminato dopo l'autenticazione) |

Entrambi sono in `.gitignore` tramite la regola `secrets/`.

---

## Setup (una tantum)

### 1. Installa le dipendenze Node.js

```bash
cd scripts/whatsapp_mcp
npm install
```

Questo installa `whatsapp-web.js`, `puppeteer` (include Chromium ~300MB) e `qrcode-terminal`.

### 2. Registra il server (da fare fare all'agente)

```
register_mcp(
  name="whatsapp",
  transport="stdio",
  command="node",
  args=["scripts/whatsapp_mcp/index.js"]
)
```

### 3. Prima autenticazione

```
mcp__whatsapp__whatsapp_status()
# → QR_READY

mcp__whatsapp__whatsapp_get_qr()
# → mostra il QR, scansionarlo con il telefono
```

---

## Esempi d'uso

### Vedere le chat recenti

```
mcp__whatsapp__whatsapp_list_chats(max_chats=10)
```

### Leggere gli ultimi messaggi di un gruppo

```
mcp__whatsapp__whatsapp_get_messages(
  chat_id="1234567890-9876543210@g.us",
  limit=50
)
```

### Paginare lo storico (messaggi più vecchi)

`offset` salta i messaggi più recenti, esponendo la finestra precedente:

```
# Ultimi 20 messaggi
whatsapp_get_messages(chat_id="...", limit=20, offset=0)

# Messaggi 21–40 (precedenti)
whatsapp_get_messages(chat_id="...", limit=20, offset=20)

# Messaggi 41–60 (ancora più vecchi)
whatsapp_get_messages(chat_id="...", limit=20, offset=40)
```

Limite: `limit + offset` non può superare 200 in una singola chiamata (vincolo di `fetchMessages`).

### Trovare il contatto di qualcuno non in chat recenti

```
mcp__whatsapp__whatsapp_search_contacts(query="Luca")
# → Luca Rossi [contact] | ID: 393331234567@c.us
```

### Cercare cosa si è detto su un argomento

```
mcp__whatsapp__whatsapp_search_messages(query="riunione lunedì")
```

### Inviare un messaggio

```
mcp__whatsapp__whatsapp_send_message(
  chat_id="393331234567@c.us",
  message="Ciao! Ci sei?"
)
```

---

## Stati di connessione

| Stato | Significato | Cosa fare |
|-------|-------------|-----------|
| `INITIALIZING` | Browser in avvio, sessione in caricamento | Aspettare qualche secondo |
| `QR_READY` | Serve scansione QR | Chiamare `whatsapp_get_qr` e scansionare |
| `AUTHENTICATED` | QR scansionato, sessione in creazione | Aspettare (→ READY automatico) |
| `READY` | Operativo | Tutti i tool disponibili |
| `DISCONNECTED` | Connessione persa | Controllare status, riavviare se necessario |

---

## Abilita / Disabilita

### Disabilita (quando non serve)

```
toggle_mcp(name="whatsapp", enabled=false)
restart
```

### Riabilita

```
toggle_mcp(name="whatsapp", enabled=true)
restart
```

---

## Dipendenze

| Pacchetto | Versione | Scopo |
|-----------|----------|-------|
| `whatsapp-web.js` | ^1.23.0 | Client WhatsApp Web |
| `puppeteer` | ^23.0.0 | Chromium headless (incluso nel pacchetto) |
| `qrcode-terminal` | ^0.12.0 | Genera QR ASCII |

**Requisiti sistema:**
- Node.js ≥ 18
- ~500MB di spazio per Puppeteer/Chromium
- Un processo Chromium in background mentre il server è attivo

---

## Errori comuni

| Errore | Causa | Soluzione |
|--------|-------|-----------|
| `whatsapp-web.js not found` | `npm install` non eseguito | `cd scripts/whatsapp_mcp && npm install` |
| `WhatsApp not ready (status: INITIALIZING)` | Server appena avviato | Aspettare 15-30 secondi |
| `WhatsApp not ready (status: QR_READY)` | Sessione scaduta/non esistente | Chiamare `whatsapp_get_qr` e scansionare |
| `WhatsApp not ready (status: DISCONNECTED)` | Connessione persa | Riavviare il server |
| Chat ID non trovato | ID errato | Usare `whatsapp_list_chats` per ottenere gli ID corretti |

---

## Protocollo

Implementa JSON-RPC 2.0 over stdio (stesso pattern di gmail e gcal):
- **Richieste:** JSON su stdin (una per riga)
- **Risposte:** JSON su stdout
- **Log:** stderr con prefisso `[whatsapp_mcp]`

Metodi supportati: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`

---

## Quando aggiornare questo file

- Nuovi tool aggiunti al server
- Cambio percorsi session/QR in `secrets/`
- Nuovi stati di connessione
- Cambio versione dipendenze
