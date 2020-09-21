Ext.define('PBS.DataStoreEdit', {
    extend: 'Proxmox.window.Edit',
    alias: 'widget.pbsDataStoreEdit',
    mixins: ['Proxmox.Mixin.CBind'],


    onlineHelp: 'datastore_intro',

    subject: gettext('Datastore'),
    isAdd: true,

    bodyPadding: 0,

    cbindData: function(initialConfig) {
	var me = this;

	let name = initialConfig.name;
	let baseurl = '/api2/extjs/config/datastore';

	me.isCreate = !name;
	if (!me.isCreate) {
	    me.defaultFocus = 'textfield[name=comment]';
	}
	me.url = name ? baseurl + '/' + name : baseurl;
	me.method = name ? 'PUT' : 'POST';
	me.autoLoad = !!name;
	return {};
    },

    items: {
	xtype: 'tabpanel',
	bodyPadding: 10,
	items: [
	    {
		title: gettext('General'),
		xtype: 'inputpanel',
		column1: [
		    {
			xtype: 'pmxDisplayEditField',
			cbind: {
			    editable: '{isCreate}',
			},
			name: 'name',
			allowBlank: false,
			fieldLabel: gettext('Name'),
		    },
		    {
			xtype: 'pmxDisplayEditField',
			cbind: {
			    editable: '{isCreate}',
			},
			name: 'path',
			allowBlank: false,
			fieldLabel: gettext('Backing Path'),
			emptyText: gettext('An absolute path'),
		    },
		],
		column2: [
		    {
			xtype: 'pbsCalendarEvent',
			name: 'gc-schedule',
			fieldLabel: gettext("GC Schedule"),
			emptyText: gettext('none'),
			cbind: {
			    deleteEmpty: '{!isCreate}',
			},
		    },
		    {
			xtype: 'pbsCalendarEvent',
			name: 'prune-schedule',
			fieldLabel: gettext("Prune Schedule"),
			emptyText: gettext('none'),
			cbind: {
			    deleteEmpty: '{!isCreate}',
			},
		    },
		{
			xtype: 'pbsCalendarEvent',
			name: 'verify-schedule',
			fieldLabel: gettext("Verify Schedule"),
			emptyText: gettext('none'),
			cbind: {
			    deleteEmpty: '{!isCreate}',
			},
		    },
		],
		columnB: [
		    {
			xtype: 'textfield',
			name: 'comment',
			fieldLabel: gettext('Comment'),
		    },
		],
	    },
	    {
		title: gettext('Prune Options'),
		xtype: 'inputpanel',
		column1: [
		    {
			xtype: 'proxmoxintegerfield',
			fieldLabel: gettext('Keep Last'),
			name: 'keep-last',
			cbind: {
			    deleteEmpty: '{!isCreate}',
			},
			minValue: 1,
			allowBlank: true,
		    },
		    {
			xtype: 'proxmoxintegerfield',
			fieldLabel: gettext('Keep Daily'),
			name: 'keep-daily',
			cbind: {
			    deleteEmpty: '{!isCreate}',
			},
			minValue: 1,
			allowBlank: true,
		    },
		    {
			xtype: 'proxmoxintegerfield',
			fieldLabel: gettext('Keep Monthly'),
			name: 'keep-monthly',
			cbind: {
			    deleteEmpty: '{!isCreate}',
			},
			minValue: 1,
			allowBlank: true,
		    },
		],
		column2: [
		    {
			xtype: 'proxmoxintegerfield',
			fieldLabel: gettext('Keep Hourly'),
			name: 'keep-hourly',
			cbind: {
			    deleteEmpty: '{!isCreate}',
			},
			minValue: 1,
			allowBlank: true,
		    },
		    {
			xtype: 'proxmoxintegerfield',
			fieldLabel: gettext('Keep Weekly'),
			name: 'keep-weekly',
			cbind: {
			    deleteEmpty: '{!isCreate}',
			},
			minValue: 1,
			allowBlank: true,
		    },
		    {
			xtype: 'proxmoxintegerfield',
			fieldLabel: gettext('Keep Yearly'),
			name: 'keep-yearly',
			cbind: {
			    deleteEmpty: '{!isCreate}',
			},
			minValue: 1,
			allowBlank: true,
		    },
		],
	    },
	],
    },
});
