/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */
import {
  COMMAND_CLOSETAB,
  COMMAND_CLOSETAB_TAIL,
  SCOPE_OLD_SYNC,
  log,
} from "resource://gre/modules/FxAccountsCommon.sys.mjs";
import { clearTimeout, setTimeout } from "resource://gre/modules/Timer.sys.mjs";

const lazy = {};

ChromeUtils.defineESModuleGetters(lazy, {
  BulkKeyBundle: "resource://services-sync/keys.sys.mjs",
  CryptoWrapper: "resource://services-sync/record.sys.mjs",
  PushCrypto: "resource://gre/modules/PushCrypto.sys.mjs",
});
/**
 * Remote Tab Management is built on-top of device commands and handles 
 * actions a client wants to perform on tabs found on other devices
 * This class is very similar to the Send Tab component in FxAccountsCommands
 *
 * Devices exchange keys wrapped in the oldsync key between themselves (getEncryptedRemoteTabKeys)
 * during the device registration flow. The FxA server can theoretically never
 * retrieve the send tab keys since it doesn't know the oldsync key.
 *
 * Note about the keys:
 * The server has the `pushPublicKey`. The FxA server encrypt the remote-tab payload again using the
 * push keys - after the client has encrypted the payload using the remote-tab keys.
 * The push keys are different from the remote-tab keys. The FxA server uses
 * the push keys to deliver the tabs using same mechanism we use for web-push.
 * However, clients use the remote-tab keys for end-to-end encryption.
 */
export class RemoteTabs {
  constructor(commands, fxAccountsInternal) {
    this._commands = commands;
    this._fxai = fxAccountsInternal;
    this.closedTabsDict = new Map();
    // pushes happen per device, making a timer per device makes sending
    // the pushes a little more sane 
    this.timers = new Map(); 
  }

  /**
   * Sending a push everytime the user wants to close a tab on a remote device
   * could lead to excessive notifications to the users device, push throttling, etc
   * so we add the tabs to a queue and have a timer that sends the push after a certain
   * amount of "inactivity"
   */
  addRemoteTabsToQueue(targetDevice, tab) {
    if (this.closedTabsDict.has(targetDevice.id)) {
      this.closedTabsDict.get(targetDevice.id).tabs.push(tab);
    } else {
      this.closedTabsDict.set(targetDevice.id, { device: targetDevice, tabs: [tab] });
    }

    // extend the timer
    this._refreshPushTimer(targetDevice.id);
  };

  _refreshPushTimer(deviceId) {
  // If the user is still performing "actions" for this device
  // reset the timer to send the push
    if (this.timers.has(deviceId)) {
      clearTimeout(this.timers.get(deviceId));
    }

    const timerId = setTimeout(async () => {
      let { device, tabs } = this.closedTabsDict.get(deviceId);
      // send a push notification for this specific device
      await this.sendRemoteTabClosePush(device, tabs);

      // Clear the timer
      this.timers.delete(deviceId);
      // We also need to locally store the tabs we sent so the user doesn't
      // see these anymore
      this.closedTabsDict.delete(deviceId);
    }, 6000); // 6 seconds, maybe use a pref here?

    // Store the new timer with the device
    this.timers.set(deviceId, timerId);
  };

  /**
   * @param {Object[]} - An array of objects of  
   * @param {Device} to - Device object (typically returned by fxAccounts.getDevicesList()).
   * @param {Object} tab
   * @param {string} tab.url
   */
  async sendRemoteTabClosePush(target, tabs) {
    log.info(`Sending tab closures to ${target} device.`);
    const flowID = this._fxai.telemetry.generateFlowID();
    const encoder = new TextEncoder();
    // right now tabs is just an array of urls
    const data = { urls: tabs };
    try {
      const streamID = this._fxai.telemetry.generateFlowID();
      const targetData = Object.assign({ flowID, streamID }, data);
      const bytes = encoder.encode(JSON.stringify(targetData));
      const encrypted = await this._encrypt(bytes, target);
      // FxA expects an object as the payload, but we only have a single encrypted string; wrap it.
      // If you add any plaintext items to this payload, please carefully consider the privacy implications
      // of revealing that data to the FxA server.
      const payload = { encrypted };
      await this._commands.invoke(COMMAND_CLOSETAB, target, payload);
      this._fxai.telemetry.recordEvent(
        "command-sent",
        COMMAND_CLOSETAB_TAIL,
        this._fxai.telemetry.sanitizeDeviceId(target.id),
        { flowID, streamID }
      );
    } catch (error) {
      // We should also show the user there was some kind've error
      log.error("Error while invoking a send tab command.", error);
    }
  }

  // Returns true if the target device is compatible with FxA Commands Send tab.
  isDeviceCompatible(device) {
    let pref = Services.prefs.getBoolPref(
      "identity.fxaccounts.commands.remoteTabManagement.enabled",
      false
    );
    return (
      pref &&
      device.availableCommands &&
      device.availableCommands[COMMAND_CLOSETAB]
    );
  }

  // Handle incoming remote tab payload, called by FxAccountsCommands.
  async handleRemoteTabClose(senderID, { encrypted }, reason) {
    const bytes = await this._decrypt(encrypted);
    const decoder = new TextDecoder("utf8");
    const data = JSON.parse(decoder.decode(bytes));
    const { flowID, streamID, urls } = data;
    // `flowID` and `streamID` are in the top-level of the JSON, `actions` is
    // an array of "tabs"
    this._fxai.telemetry.recordEvent(
      "command-received",
      COMMAND_CLOSETAB_TAIL,
      this._fxai.telemetry.sanitizeDeviceId(senderID),
      { flowID, streamID, reason }
    );

    return {
      urls,
    };
  }

  async _encrypt(bytes, device) {
    let bundle = device.availableCommands[COMMAND_CLOSETAB];
    if (!bundle) {
      throw new Error(`Device ${device.id} does not have remote tab keys.`);
    }
    const oldsyncKey = await this._fxai.keys.getKeyForScope(SCOPE_OLD_SYNC);
    // Older clients expect this to be hex, due to pre-JWK sync key ids :-(
    const ourKid = this._fxai.keys.kidAsHex(oldsyncKey);
    const { kid: theirKid } = JSON.parse(
      device.availableCommands[COMMAND_CLOSETAB]
    );
    if (theirKid != ourKid) {
      throw new Error("Target Remote Tab key ID is different from ours");
    }
    const json = JSON.parse(bundle);
    const wrapper = new lazy.CryptoWrapper();
    wrapper.deserialize({ payload: json });
    const syncKeyBundle = lazy.BulkKeyBundle.fromJWK(oldsyncKey);
    let { publicKey, authSecret } = await wrapper.decrypt(syncKeyBundle);
    authSecret = urlsafeBase64Decode(authSecret);
    publicKey = urlsafeBase64Decode(publicKey);

    const { ciphertext: encrypted } = await lazy.PushCrypto.encrypt(
      bytes,
      publicKey,
      authSecret
    );
    return urlsafeBase64Encode(encrypted);
  }

  async _getPersistedRemoteTabKeys() {
    const { device } = await this._fxai.getUserAccountData(["device"]);
    return device && device.remoteTabKeys;
  }

  async _decrypt(ciphertext) {
    let { privateKey, publicKey, authSecret } =
      await this._getPersistedRemoteTabKeys();
    publicKey = urlsafeBase64Decode(publicKey);
    authSecret = urlsafeBase64Decode(authSecret);
    ciphertext = new Uint8Array(urlsafeBase64Decode(ciphertext));
    return lazy.PushCrypto.decrypt(
      privateKey,
      publicKey,
      authSecret,
      // The only Push encoding we support.
      { encoding: "aes128gcm" },
      ciphertext
    );
  }

  async _generateAndPersistRemoteTabKeys() {
    let [publicKey, privateKey] = await lazy.PushCrypto.generateKeys();
    publicKey = urlsafeBase64Encode(publicKey);
    let authSecret = lazy.PushCrypto.generateAuthenticationSecret();
    authSecret = urlsafeBase64Encode(authSecret);
    const remoteTabKeys = {
      publicKey,
      privateKey,
      authSecret,
    };
    await this._fxai.withCurrentAccountState(async state => {
      const { device } = await state.getUserAccountData(["device"]);
      await state.updateUserAccountData({
        device: {
          ...device,
          remoteTabKeys,
        },
      });
    });
    return remoteTabKeys;
  }

  async _getPersistedEncryptedRemoteTabKey() {
    const { encryptedRemoteTabKeys } = await this._fxai.getUserAccountData([
      "encryptedRemoteTabKeys",
    ]);
    return encryptedRemoteTabKeys;
  }

  async _generateAndPersistEncryptedRemoteTabKey() {
    let remoteTabKeys = await this._getPersistedRemoteTabKeys();
    if (!remoteTabKeys) {
      log.info("Could not find remotetab keys, generating them");
      remoteTabKeys = await this._generateAndPersistRemoteTabKeys();
    }
    // Strip the private key from the bundle to encrypt.
    const keyToEncrypt = {
      publicKey: remoteTabKeys.publicKey,
      authSecret: remoteTabKeys.authSecret,
    };
    if (!(await this._fxai.keys.canGetKeyForScope(SCOPE_OLD_SYNC))) {
      log.info("Can't fetch keys, so unable to determine remotetab keys");
      return null;
    }
    let oldsyncKey;
    try {
      oldsyncKey = await this._fxai.keys.getKeyForScope(SCOPE_OLD_SYNC);
    } catch (ex) {
      log.warn(
        "Failed to fetch keys, so unable to determine remotetab keys",
        ex
      );
      return null;
    }
    const wrapper = new lazy.CryptoWrapper();
    wrapper.cleartext = keyToEncrypt;
    const keyBundle = lazy.BulkKeyBundle.fromJWK(oldsyncKey);
    await wrapper.encrypt(keyBundle);
    const encryptedRemoteTabKeys = JSON.stringify({
      // This is expected in hex, due to pre-JWK sync key ids :-(
      kid: this._fxai.keys.kidAsHex(oldsyncKey),
      IV: wrapper.IV,
      hmac: wrapper.hmac,
      ciphertext: wrapper.ciphertext,
    });
    await this._fxai.withCurrentAccountState(async state => {
      await state.updateUserAccountData({
        encryptedRemoteTabKeys,
      });
    });
    return encryptedRemoteTabKeys;
  }

  async getEncryptedRemoteTabKeys() {
    let encryptedRemoteTabKeys =
      await this._getPersistedEncryptedRemoteTabKey();
    const remoteTabKeys = await this._getPersistedRemoteTabKeys();
    if (!encryptedRemoteTabKeys || !remoteTabKeys) {
      log.info("Generating and persisting encrypted remotetab keys");
      // `_generateAndPersistEncryptedKeys` requires the sync key
      // which cannot be accessed if the login manager is locked
      // (i.e when the primary password is locked) or if the sync keys
      // aren't accessible (account isn't verified)
      // so this function could fail to retrieve the keys
      // however, device registration will trigger when the account
      // is verified, so it's OK
      // Note that it's okay to persist those keys, because they are
      // already persisted in plaintext and the encrypted bundle
      // does not include the sync-key (the sync key is used to encrypt
      // it though)
      encryptedRemoteTabKeys =
        await this._generateAndPersistEncryptedRemoteTabKey();
    }
    return encryptedRemoteTabKeys;
  }
}

function urlsafeBase64Encode(buffer) {
  return ChromeUtils.base64URLEncode(new Uint8Array(buffer), { pad: false });
}

function urlsafeBase64Decode(str) {
  return ChromeUtils.base64URLDecode(str, { padding: "reject" });
}
