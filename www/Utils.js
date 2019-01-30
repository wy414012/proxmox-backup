/*global Proxmox */
Ext.ns('PBS');

console.log("Starting Backup Server GUI");

Ext.define('PBS.Utils', {
    singleton: true,

    updateLoginData: function(data) {
	Proxmox.CSRFPreventionToken = data.CSRFPreventionToken;
	Proxmox.UserName = data.username;
	Ext.util.Cookies.set('PBSAuthCookie', data.ticket, null, '/', null, true );
    },

    constructor: function() {
	var me = this;

	// do whatever you want here
    }
});
